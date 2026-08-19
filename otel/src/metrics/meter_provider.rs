use crate::{
    metrics::{
        memory_exporter::MEMORY_EXPORTER,
        meter::{MeterClass, MeterState},
    },
    request,
    runtime::init_tokio_runtime,
    util,
};
use once_cell::sync::Lazy;
use opentelemetry::{InstrumentationScope, KeyValue, metrics::MeterProvider as _};
use opentelemetry_otlp::{MetricExporter as OtlpMetricExporter, Protocol, WithExportConfig};
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider},
};
use opentelemetry_stdout::MetricExporter as StdoutMetricExporter;
use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    collections::hash_map::DefaultHasher,
    convert::Infallible,
    env,
    hash::{Hash, Hasher},
    process,
    sync::{Arc, Mutex},
};

pub const METER_PROVIDER_CLASS_NAME: &str = r"OpenTelemetry\API\Metrics\MeterProvider";
pub type MeterProviderClass = StateClass<()>;
pub type ProviderKey = (u32, String);

pub struct NativeMeterProvider {
    pub sdk: SdkMeterProvider,
    pub enabled: bool,
    pub key: ProviderKey,
}

static METER_PROVIDERS: Lazy<Mutex<HashMap<ProviderKey, Arc<NativeMeterProvider>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

thread_local! {
    static REQUEST_CONFIG_HASH: RefCell<Option<String>> = const { RefCell::new(None) };
}

static NOOP_METER_PROVIDER: Lazy<Arc<NativeMeterProvider>> = Lazy::new(|| {
    Arc::new(NativeMeterProvider {
        sdk: SdkMeterProvider::builder()
            .with_resource(Resource::builder_empty().build())
            .build(),
        enabled: false,
        key: (process::id(), "noop".to_string()),
    })
});

/// Cache the effective configuration hash while a request is active. Besides
/// avoiding repeated environment scans, this makes provider selection stable
/// if application code calls `putenv()` after RINIT.
pub fn begin_request() {
    let hash = compute_config_hash();
    REQUEST_CONFIG_HASH.with(|cache| *cache.borrow_mut() = Some(hash));
}

pub fn end_request() {
    REQUEST_CONFIG_HASH.with(|cache| *cache.borrow_mut() = None);
}

fn provider_key() -> ProviderKey {
    let hash = REQUEST_CONFIG_HASH
        .with(|cache| cache.borrow().clone())
        .unwrap_or_else(compute_config_hash);
    (process::id(), hash)
}

fn compute_config_hash() -> String {
    let mut hasher = DefaultHasher::new();
    for name in [
        "OTEL_SERVICE_NAME",
        "OTEL_RESOURCE_ATTRIBUTES",
        "OTEL_METRICS_EXPORTER",
        "OTEL_EXPORTER_OTLP_PROTOCOL",
        "OTEL_EXPORTER_OTLP_METRICS_PROTOCOL",
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
        "OTEL_EXPORTER_OTLP_HEADERS",
        "OTEL_EXPORTER_OTLP_METRICS_HEADERS",
        "OTEL_EXPORTER_OTLP_COMPRESSION",
        "OTEL_EXPORTER_OTLP_METRICS_COMPRESSION",
        "OTEL_EXPORTER_OTLP_TIMEOUT",
        "OTEL_EXPORTER_OTLP_METRICS_TIMEOUT",
        "OTEL_EXPORTER_OTLP_CERTIFICATE",
        "OTEL_EXPORTER_OTLP_METRICS_CERTIFICATE",
        "OTEL_EXPORTER_OTLP_CLIENT_KEY",
        "OTEL_EXPORTER_OTLP_METRICS_CLIENT_KEY",
        "OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE",
        "OTEL_EXPORTER_OTLP_METRICS_CLIENT_CERTIFICATE",
        "OTEL_EXPORTER_OTLP_INSECURE",
        "OTEL_EXPORTER_OTLP_METRICS_INSECURE",
        "OTEL_METRIC_EXPORT_INTERVAL",
        "OTEL_METRIC_EXPORT_TIMEOUT",
        "OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE",
    ] {
        name.hash(&mut hasher);
        env::var(name).unwrap_or_default().hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn resource() -> Resource {
    Resource::builder()
        .with_attribute(KeyValue::new("telemetry.sdk.language", "php"))
        .with_attribute(KeyValue::new("telemetry.sdk.name", "ext-otel"))
        .with_attribute(KeyValue::new(
            "telemetry.sdk.version",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_attribute(KeyValue::new(
            "process.runtime.name",
            util::get_sapi_module_name(),
        ))
        .with_attribute(KeyValue::new(
            "process.runtime.version",
            util::get_php_version(),
        ))
        .with_attribute(KeyValue::new("process.pid", process::id().to_string()))
        .with_attribute(KeyValue::new(
            "host.name",
            hostname::get()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
        ))
        .build()
}

fn build_provider(key: ProviderKey) -> Arc<NativeMeterProvider> {
    let exporter = env::var("OTEL_METRICS_EXPORTER").unwrap_or_else(|_| "otlp".to_string());
    let builder = SdkMeterProvider::builder().with_resource(resource());

    let (sdk, enabled) = match exporter.as_str() {
        "none" => (builder.build(), false),
        "memory" => {
            MEMORY_EXPORTER.reset();
            let reader = PeriodicReader::builder(MEMORY_EXPORTER.clone()).build();
            (builder.with_reader(reader).build(), true)
        }
        "console" => {
            let reader = PeriodicReader::builder(StdoutMetricExporter::default()).build();
            (builder.with_reader(reader).build(), true)
        }
        "otlp" => {
            let protocol = env::var("OTEL_EXPORTER_OTLP_METRICS_PROTOCOL")
                .or_else(|_| env::var("OTEL_EXPORTER_OTLP_PROTOCOL"))
                .unwrap_or_else(|_| "grpc".to_string());
            let result = if protocol == "http/protobuf" {
                OtlpMetricExporter::builder()
                    .with_http()
                    .with_protocol(Protocol::HttpBinary)
                    .build()
            } else {
                match init_tokio_runtime() {
                    Ok(runtime) => runtime
                        .block_on(async { OtlpMetricExporter::builder().with_tonic().build() }),
                    Err(error) => {
                        tracing::warn!("failed to create metrics export runtime: {error}");
                        return Arc::new(NativeMeterProvider {
                            sdk: builder.build(),
                            enabled: false,
                            key,
                        });
                    }
                }
            };
            match result {
                Ok(exporter) => {
                    let reader = PeriodicReader::builder(exporter).build();
                    (builder.with_reader(reader).build(), true)
                }
                Err(error) => {
                    tracing::warn!(
                        "failed to create OTLP metrics exporter: {error:?}; using no-op metrics provider"
                    );
                    (builder.build(), false)
                }
            }
        }
        other => {
            tracing::warn!(
                "unsupported OTEL_METRICS_EXPORTER={other:?}; using no-op metrics provider"
            );
            (builder.build(), false)
        }
    };

    Arc::new(NativeMeterProvider { sdk, enabled, key })
}

pub fn init_once() {
    let key = provider_key();
    let mut providers = METER_PROVIDERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if providers.contains_key(&key) {
        return;
    }
    let provider = build_provider(key.clone());
    providers.insert(key, provider);
}

pub fn get_meter_provider() -> Arc<NativeMeterProvider> {
    if request::is_disabled() {
        return NOOP_METER_PROVIDER.clone();
    }
    let key = provider_key();
    METER_PROVIDERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&key)
        .cloned()
        .unwrap_or_else(|| {
            tracing::warn!(
                "no meter provider initialized for pid {}; using no-op",
                key.0
            );
            NOOP_METER_PROVIDER.clone()
        })
}

pub fn force_flush() {
    let key = provider_key();
    let provider = METER_PROVIDERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&key)
        .filter(|provider| provider.enabled)
        .cloned();
    if let Some(provider) = provider {
        if let Err(error) = provider.sdk.force_flush() {
            tracing::warn!("metrics force flush failed: {error}");
        }
    }
}

pub fn shutdown() {
    let pid = process::id();
    crate::metrics::meter::remove_async_caches(pid);
    let providers = {
        let mut map = METER_PROVIDERS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let keys = map
            .keys()
            .filter(|(provider_pid, _)| *provider_pid == pid)
            .cloned()
            .collect::<Vec<_>>();
        keys.into_iter()
            .filter_map(|key| map.remove(&key))
            .collect::<Vec<_>>()
    };
    for provider in providers {
        if let Err(error) = provider.sdk.shutdown() {
            tracing::warn!("metrics provider shutdown failed: {error}");
        }
    }
}

pub fn make_meter_provider_class(interface: Interface, meter_class: MeterClass) -> ClassEntity<()> {
    let mut class = ClassEntity::new_with_default_state_constructor(METER_PROVIDER_CLASS_NAME);
    class.set_final();
    class.implements(interface);
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });
    class
        .add_method("getMeter", Visibility::Public, move |_, arguments| {
            let name = util::arg(arguments, 0)?
                .expect_z_str()?
                .to_str()?
                .to_string();
            let version = arguments
                .get(1)
                .and_then(|value| value.as_z_str())
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let schema_url = arguments
                .get(2)
                .and_then(|value| value.as_z_str())
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let attributes = match arguments.get(3) {
                Some(value) => {
                    util::zval_iterable_to_key_value_vec(value, util::AttributeDestination::Scope)?
                }
                None => Vec::new(),
            };

            let provider = get_meter_provider();
            let mut scope = InstrumentationScope::builder(name);
            if let Some(version) = version {
                scope = scope.with_version(version);
            }
            if let Some(schema_url) = schema_url {
                scope = scope.with_schema_url(schema_url);
            }
            if !attributes.is_empty() {
                scope = scope.with_attributes(attributes);
            }
            let scope = scope.build();
            let scope_key = format!(
                "{}\n{}\n{}",
                scope.name(),
                scope.version().as_deref().unwrap_or(""),
                scope.schema_url().as_deref().unwrap_or("")
            );
            let meter = provider.sdk.meter_with_scope(scope);
            let mut object = meter_class.init_object()?;
            *object.as_mut_state() = MeterState {
                meter: Some(meter),
                enabled: provider.enabled,
                provider_key: provider.key.clone(),
                scope_key,
            };
            Ok::<_, phper::Error>(object)
        })
        .argument(Argument::new("name").with_type_hint(ArgumentTypeHint::String))
        .argument(
            Argument::new("version")
                .with_type_hint(ArgumentTypeHint::String)
                .allow_null()
                .with_default_value("NULL"),
        )
        .argument(
            Argument::new("schemaUrl")
                .with_type_hint(ArgumentTypeHint::String)
                .allow_null()
                .with_default_value("NULL"),
        )
        .argument(
            Argument::new("attributes")
                .with_type_hint(ArgumentTypeHint::Iterable)
                .with_default_value("[]"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\API\Metrics\MeterInterface".to_string(),
        )));
    class
}
