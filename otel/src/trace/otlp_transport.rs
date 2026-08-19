//! OTLP transport settings that opentelemetry-otlp does not derive from the
//! environment itself: TLS trust and client identity, scheme-less gRPC endpoints
//! with `OTEL_EXPORTER_OTLP_INSECURE`, sanitised request headers and
//! compression. For every variable the trace-specific `OTEL_EXPORTER_OTLP_TRACES_*`
//! form wins over the generic one; empty values count as unset.

use http::{HeaderMap, HeaderName, HeaderValue};
use opentelemetry_http::{Bytes, HttpClient, HttpError, Request, Response};
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceResponse;
use prost::Message;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use std::{
    env,
    ffi::OsString,
    fs,
    str::FromStr,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tonic::{
    metadata::MetadataMap,
    transport::{Certificate, ClientTlsConfig, Identity},
};
use url::Url;

const DEFAULT_GRPC_ENDPOINT: &str = "http://localhost:4317";
const DEFAULT_HTTP_ENDPOINT: &str = "http://localhost:4318";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Grpc,
    HttpProtobuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
}

#[derive(Debug)]
pub struct ClientIdentity {
    pub certificate_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
}

/// TLS material for an `https` endpoint. Without a CA the bundled webpki roots
/// are trusted; with one, only that CA bundle is.
#[derive(Debug)]
pub struct TlsMaterial {
    pub ca_pem: Option<Vec<u8>>,
    pub client_identity: Option<ClientIdentity>,
}

#[derive(Debug)]
pub struct TransportSettings {
    pub protocol: Protocol,
    /// Effective endpoint; a scheme-less gRPC `host:port` is normalised to
    /// `https://` (or `http://` under `OTEL_EXPORTER_OTLP_INSECURE=true`).
    pub endpoint: String,
    pub tls: Option<TlsMaterial>,
    pub headers: Vec<(HeaderName, HeaderValue)>,
    pub compression: Compression,
}

impl TransportSettings {
    /// Resolves the transport settings; the error is a complete diagnostic
    /// (without credentials) and means the caller must fall back to a no-op
    /// provider.
    pub fn from_env(protocol: Protocol) -> Result<Self, String> {
        let endpoint = resolve_endpoint(protocol)?;
        let tls = resolve_tls(&endpoint)?;
        let compression = resolve_compression()?;
        Ok(Self {
            protocol,
            endpoint: endpoint.raw,
            tls,
            headers: resolve_headers(),
            compression,
        })
    }
}

fn trace_or_generic(suffix: &str) -> Option<(String, String)> {
    [
        format!("OTEL_EXPORTER_OTLP_TRACES_{suffix}"),
        format!("OTEL_EXPORTER_OTLP_{suffix}"),
    ]
    .into_iter()
    .find_map(|name| {
        let value = env::var(&name).ok()?;
        let value = value.trim();
        (!value.is_empty()).then(|| (name, value.to_string()))
    })
}

struct Endpoint {
    raw: String,
    url: Url,
}

fn resolve_endpoint(protocol: Protocol) -> Result<Endpoint, String> {
    let Some((name, configured)) = trace_or_generic("ENDPOINT") else {
        let raw = match protocol {
            Protocol::Grpc => DEFAULT_GRPC_ENDPOINT,
            Protocol::HttpProtobuf => DEFAULT_HTTP_ENDPOINT,
        };
        return Ok(Endpoint {
            raw: raw.to_string(),
            url: Url::parse(raw).map_err(|error| error.to_string())?,
        });
    };
    // Per the OTLP exporter specification OTEL_EXPORTER_OTLP_INSECURE only applies to
    // gRPC endpoints given without a scheme (`host:port` authority form); OTLP/HTTP
    // always uses the URL scheme. Anything else without a scheme is a typo, not a host.
    let raw = if protocol == Protocol::Grpc && !configured.contains("://") {
        if !is_authority(&configured) {
            return Err(format!("Invalid OTLP endpoint in {name}"));
        }
        let insecure = trace_or_generic("INSECURE")
            .is_some_and(|(_, value)| value.eq_ignore_ascii_case("true"));
        format!(
            "{}://{configured}",
            if insecure { "http" } else { "https" }
        )
    } else {
        configured
    };
    match Url::parse(&raw) {
        Ok(url)
            if matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none() =>
        {
            Ok(Endpoint { raw, url })
        }
        _ => Err(format!("Invalid OTLP endpoint in {name}")),
    }
}

/// `host:port` or `[v6]:port` with a numeric port and no path.
fn is_authority(value: &str) -> bool {
    let Some((host, port)) = value.rsplit_once(':') else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    !host.is_empty()
        && !host.contains('/')
        && !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
}

enum PemKind {
    Certificate,
    PrivateKey,
}

fn read_pem(suffix: &str, kind: PemKind) -> Result<Option<Vec<u8>>, String> {
    let Some((name, path)) = trace_or_generic(suffix) else {
        return Ok(None);
    };
    let bytes = fs::read(&path).map_err(|error| format!("{name}: cannot read {path}: {error}"))?;
    let valid = match kind {
        PemKind::Certificate => CertificateDer::pem_slice_iter(&bytes)
            .collect::<Result<Vec<_>, _>>()
            .is_ok_and(|certificates| !certificates.is_empty()),
        PemKind::PrivateKey => PrivateKeyDer::from_pem_slice(&bytes).is_ok(),
    };
    if !valid {
        let expected = match kind {
            PemKind::Certificate => "certificate",
            PemKind::PrivateKey => "private key",
        };
        return Err(format!("{name}: {path} does not contain a PEM {expected}"));
    }
    Ok(Some(bytes))
}

fn resolve_tls(endpoint: &Endpoint) -> Result<Option<TlsMaterial>, String> {
    if endpoint.url.scheme() != "https" {
        if ["CERTIFICATE", "CLIENT_KEY", "CLIENT_CERTIFICATE"]
            .iter()
            .any(|suffix| trace_or_generic(suffix).is_some())
        {
            tracing::warn!(
                "OTEL_EXPORTER_OTLP_*CERTIFICATE/CLIENT_KEY settings are ignored for a non-https OTLP endpoint"
            );
        }
        return Ok(None);
    }
    let ca_pem = read_pem("CERTIFICATE", PemKind::Certificate)?;
    let certificate_pem = read_pem("CLIENT_CERTIFICATE", PemKind::Certificate)?;
    let key_pem = read_pem("CLIENT_KEY", PemKind::PrivateKey)?;
    let client_identity = match (certificate_pem, key_pem) {
        (Some(certificate_pem), Some(key_pem)) => Some(ClientIdentity {
            certificate_pem,
            key_pem,
        }),
        (None, None) => None,
        (Some(_), None) => {
            return Err(
                "OTEL_EXPORTER_OTLP_CLIENT_KEY is required together with OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE"
                    .to_string(),
            );
        }
        (None, Some(_)) => {
            return Err(
                "OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE is required together with OTEL_EXPORTER_OTLP_CLIENT_KEY"
                    .to_string(),
            );
        }
    };
    Ok(Some(TlsMaterial {
        ca_pem,
        client_identity,
    }))
}

fn resolve_compression() -> Result<Compression, String> {
    match trace_or_generic("COMPRESSION") {
        None => Ok(Compression::None),
        Some((name, value)) => match value.to_ascii_lowercase().as_str() {
            "gzip" => Ok(Compression::Gzip),
            "none" => Ok(Compression::None),
            other => Err(format!("Unsupported OTLP compression {other:?} in {name}")),
        },
    }
}

/// Parses `key=value,key2=value2` (W3C-baggage style, percent-encoded values).
/// Malformed entries are skipped with a diagnostic that names the entry position
/// only, never its content; a repeated key keeps the last value.
fn resolve_headers() -> Vec<(HeaderName, HeaderValue)> {
    let Some((name, raw)) = trace_or_generic("HEADERS") else {
        return Vec::new();
    };
    let mut headers: Vec<(HeaderName, HeaderValue)> = Vec::new();
    for (position, entry) in raw.split(',').enumerate() {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        match parse_header_entry(entry) {
            Ok((key, value)) => {
                if let Some(existing) = headers.iter_mut().find(|(existing, _)| *existing == key) {
                    tracing::warn!(
                        "Header {} is repeated in {}; the last value is used",
                        key.as_str(),
                        name
                    );
                    existing.1 = value;
                } else {
                    headers.push((key, value));
                }
            }
            Err(reason) => tracing::warn!(
                "Ignoring malformed entry {} of {}: {}",
                position + 1,
                name,
                reason
            ),
        }
    }
    headers
}

fn parse_header_entry(entry: &str) -> Result<(HeaderName, HeaderValue), &'static str> {
    let (key, value) = entry.split_once('=').ok_or("missing '='")?;
    let key = key.trim();
    if key.is_empty() {
        return Err("empty header name");
    }
    let name = HeaderName::from_str(key).map_err(|_| "invalid header name")?;
    let value = value.trim();
    if value.is_empty() {
        return Err("empty header value");
    }
    let decoded = percent_decode(value).ok_or("invalid percent-encoding")?;
    let value = HeaderValue::from_bytes(&decoded).map_err(|_| "invalid header value")?;
    if value.to_str().is_err() {
        return Err("header value is not visible ASCII");
    }
    Ok((name, value))
}

fn percent_decode(value: &str) -> Option<Vec<u8>> {
    let mut decoded = Vec::with_capacity(value.len());
    let mut rest = value.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = std::str::from_utf8(tail.get(..2)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            rest = tail.get(2..)?;
        } else {
            decoded.push(byte);
            rest = tail;
        }
    }
    Some(decoded)
}

/// opentelemetry-otlp re-reads the header and compression variables while
/// building an exporter, rejects spec values such as `none`, and offers no way
/// to opt out. The extension parses them itself, so they are hidden from the
/// builder and restored immediately afterwards.
pub fn with_exporter_env_masked<T>(build: impl FnOnce() -> T) -> T {
    const MASKED: [&str; 4] = [
        "OTEL_EXPORTER_OTLP_TRACES_HEADERS",
        "OTEL_EXPORTER_OTLP_HEADERS",
        "OTEL_EXPORTER_OTLP_TRACES_COMPRESSION",
        "OTEL_EXPORTER_OTLP_COMPRESSION",
    ];
    struct Restore(Vec<(&'static str, OsString)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for (name, value) in self.0.drain(..) {
                // SAFETY: see below; the value is the one that was present before masking.
                unsafe { env::set_var(name, value) };
            }
        }
    }
    let saved: Vec<_> = MASKED
        .iter()
        .filter_map(|name| env::var_os(name).map(|value| (*name, value)))
        .collect();
    // SAFETY: providers are built on the PHP request thread, which is the only thread
    // that reads or writes the process environment (RINIT imports, dotenv); exporter
    // workers and the Tokio runtime never touch it.
    for (name, _) in &saved {
        unsafe { env::remove_var(name) };
    }
    let _restore = Restore(saved);
    build()
}

pub fn grpc_tls_config(tls: &TlsMaterial) -> ClientTlsConfig {
    let mut config = match &tls.ca_pem {
        Some(ca_pem) => ClientTlsConfig::new().ca_certificate(Certificate::from_pem(ca_pem.clone())),
        None => ClientTlsConfig::new().with_webpki_roots(),
    };
    if let Some(identity) = &tls.client_identity {
        config = config.identity(Identity::from_pem(
            identity.certificate_pem.clone(),
            identity.key_pem.clone(),
        ));
    }
    config
}

pub fn grpc_metadata(headers: &[(HeaderName, HeaderValue)]) -> MetadataMap {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        map.insert(name.clone(), value.clone());
    }
    MetadataMap::from_headers(map)
}

/// Blocking reqwest client for OTLP/HTTP. Proxy settings come from the standard
/// `HTTPS_PROXY`/`HTTP_PROXY`/`NO_PROXY` variables (reqwest default).
pub fn build_http_client(
    settings: &TransportSettings,
    timeout: Duration,
) -> Result<HttpTransport, String> {
    let mut builder = reqwest::blocking::Client::builder().timeout(timeout);
    if let Some(tls) = &settings.tls {
        if let Some(ca_pem) = &tls.ca_pem {
            builder = builder.tls_built_in_root_certs(false);
            let certificates = reqwest::Certificate::from_pem_bundle(ca_pem)
                .map_err(|error| format!("OTEL_EXPORTER_OTLP_CERTIFICATE: {error}"))?;
            for certificate in certificates {
                builder = builder.add_root_certificate(certificate);
            }
        }
        if let Some(identity) = &tls.client_identity {
            let mut pem = identity.certificate_pem.clone();
            pem.push(b'\n');
            pem.extend_from_slice(&identity.key_pem);
            let identity = reqwest::Identity::from_pem(&pem).map_err(|error| {
                format!("OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE/OTEL_EXPORTER_OTLP_CLIENT_KEY: {error}")
            })?;
            builder = builder.identity(identity);
        }
    }
    let client = builder
        .build()
        .map_err(|error| format!("cannot build OTLP HTTP client: {error}"))?;
    Ok(HttpTransport {
        client,
        headers: settings.headers.clone(),
        partial_success_logged: AtomicBool::new(false),
    })
}

/// OTLP/HTTP client: applies the sanitised headers and surfaces collector
/// partial-success responses, which opentelemetry-otlp otherwise ignores.
#[derive(Debug)]
pub struct HttpTransport {
    client: reqwest::blocking::Client,
    headers: Vec<(HeaderName, HeaderValue)>,
    partial_success_logged: AtomicBool,
}

impl HttpTransport {
    fn inspect_partial_success(&self, response: &Response<Bytes>) {
        let protobuf = response
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("application/x-protobuf"));
        if !protobuf || response.body().is_empty() {
            return;
        }
        let Ok(decoded) = ExportTraceServiceResponse::decode(response.body().as_ref()) else {
            return;
        };
        let Some(partial) = decoded.partial_success else {
            return;
        };
        if partial.rejected_spans == 0 && partial.error_message.is_empty() {
            return;
        }
        let message: String = partial.error_message.chars().take(200).collect();
        if !self.partial_success_logged.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                "OTLP collector reported partial success: rejected_spans={} message={:?}; the batch stays counted as exported and further partial-success diagnostics are suppressed",
                partial.rejected_spans,
                message
            );
        } else {
            tracing::debug!(
                "OTLP collector reported partial success: rejected_spans={} message={:?}",
                partial.rejected_spans,
                message
            );
        }
    }
}

#[async_trait::async_trait]
impl HttpClient for HttpTransport {
    async fn send_bytes(&self, mut request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        for (name, value) in &self.headers {
            request.headers_mut().insert(name.clone(), value.clone());
        }
        let response = self.client.send_bytes(request).await?;
        self.inspect_partial_success(&response);
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_less_grpc_endpoints_must_be_host_and_port() {
        assert!(is_authority("collector:4317"));
        assert!(is_authority("[::1]:4317"));
        assert!(!is_authority("not-a-url"));
        assert!(!is_authority("collector:port"));
        assert!(!is_authority("collector/path:4317"));
        assert!(!is_authority(":4317"));
    }

    #[test]
    fn header_entries_follow_the_documented_rules() {
        let (name, value) = parse_header_entry(" X-Token = Bearer%20abc%2Cdef ").unwrap();
        assert_eq!(name.as_str(), "x-token");
        assert_eq!(value.to_str().unwrap(), "Bearer abc,def");
        assert_eq!(parse_header_entry("no-equals"), Err("missing '='"));
        assert_eq!(parse_header_entry("=value"), Err("empty header name"));
        assert_eq!(parse_header_entry("key="), Err("empty header value"));
        assert_eq!(parse_header_entry("bad key=1"), Err("invalid header name"));
        assert_eq!(parse_header_entry("key=%zz"), Err("invalid percent-encoding"));
        assert_eq!(parse_header_entry("key=a%0Ab"), Err("invalid header value"));
        assert_eq!(
            parse_header_entry("key=%C3%A9"),
            Err("header value is not visible ASCII")
        );
    }
}
