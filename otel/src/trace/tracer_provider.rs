use crate::{
    module, request,
    runtime::init_tokio_runtime,
    trace::{
        batch_processor::{
            BatchMetrics, BatchMetricsSnapshot, BatchProcessorConfig, BoundedBatchSpanProcessor,
        },
        memory_exporter::MEMORY_EXPORTER,
        otlp_transport::{self, TransportSettings},
        tracer::TracerClass,
    },
    util,
};
use once_cell::sync::Lazy;
use opentelemetry::{InstrumentationScope, KeyValue, trace::TracerProvider};
use opentelemetry_otlp::{
    Compression, Protocol, SpanExporter as OtlpSpanExporter, WithExportConfig, WithHttpConfig,
    WithTonicConfig,
};
use opentelemetry_sdk::{
    Resource,
    trace::{Sampler::AlwaysOff, SdkTracerProvider, SpanExporter, TracerProviderBuilder},
};
use opentelemetry_stdout::SpanExporter as StdoutSpanExporter;
use phper::{
    arrays::ZArray,
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

const TRACER_PROVIDER_CLASS_NAME: &str = r"OpenTelemetry\API\Trace\TracerProvider";

pub type TracerProviderClass = StateClass<()>;

#[derive(Clone)]
struct ProviderEntry {
    provider: Arc<SdkTracerProvider>,
    metrics: BatchMetrics,
    shutdown_timeout: std::time::Duration,
}

static TRACER_PROVIDERS: Lazy<Mutex<HashMap<(u32, String), ProviderEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static NOOP_TRACER_PROVIDER: Lazy<Arc<SdkTracerProvider>> = Lazy::new(|| {
    Arc::new(
        SdkTracerProvider::builder()
            .with_resource(Resource::builder_empty().build())
            .with_sampler(AlwaysOff)
            .build(),
    )
});

thread_local! {
    // Configuration hash of the current request. The environment only changes
    // between RINIT (dotenv / $_SERVER import) and RSHUTDOWN (restore), so
    // hashing the configuration variables once per request instead of on every
    // provider lookup keeps getTracer() and RINIT off the env lock.
    static REQUEST_CONFIG_HASH: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Cache the effective configuration hash for the duration of a request.
pub fn begin_request() {
    let hash = compute_config_hash();
    REQUEST_CONFIG_HASH.with(|cache| *cache.borrow_mut() = Some(hash));
}

/// Forget the per-request configuration hash.
pub fn end_request() {
    REQUEST_CONFIG_HASH.with(|cache| *cache.borrow_mut() = None);
}

// Tracer provider per PID and effective non-secret configuration. The hashed
// key keeps endpoints and resource values out of diagnostics. The PID is always
// read fresh so a fork inside a request still gets its own provider.
fn get_tracer_provider_key() -> (u32, String) {
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
        "OTEL_TRACES_EXPORTER",
        "OTEL_TRACES_SAMPLER",
        "OTEL_TRACES_SAMPLER_ARG",
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
        "OTEL_EXPORTER_OTLP_PROTOCOL",
        "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL",
        "OTEL_EXPORTER_OTLP_HEADERS",
        "OTEL_EXPORTER_OTLP_TRACES_HEADERS",
        "OTEL_EXPORTER_OTLP_COMPRESSION",
        "OTEL_EXPORTER_OTLP_TRACES_COMPRESSION",
        "OTEL_EXPORTER_OTLP_TIMEOUT",
        "OTEL_EXPORTER_OTLP_TRACES_TIMEOUT",
        "OTEL_EXPORTER_OTLP_CERTIFICATE",
        "OTEL_EXPORTER_OTLP_TRACES_CERTIFICATE",
        "OTEL_EXPORTER_OTLP_CLIENT_KEY",
        "OTEL_EXPORTER_OTLP_TRACES_CLIENT_KEY",
        "OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE",
        "OTEL_EXPORTER_OTLP_TRACES_CLIENT_CERTIFICATE",
        "OTEL_EXPORTER_OTLP_INSECURE",
        "OTEL_EXPORTER_OTLP_TRACES_INSECURE",
        "OTEL_SPAN_PROCESSOR",
        "OTEL_ATTRIBUTE_COUNT_LIMIT",
        "OTEL_ATTRIBUTE_VALUE_LENGTH_LIMIT",
        "OTEL_SPAN_ATTRIBUTE_COUNT_LIMIT",
        "OTEL_SPAN_ATTRIBUTE_VALUE_LENGTH_LIMIT",
        "OTEL_EVENT_ATTRIBUTE_COUNT_LIMIT",
        "OTEL_LINK_ATTRIBUTE_COUNT_LIMIT",
        "OTEL_SPAN_EVENT_COUNT_LIMIT",
        "OTEL_SPAN_LINK_COUNT_LIMIT",
        "OTEL_PHP_ATTRIBUTE_KEY_LENGTH_LIMIT",
        "OTEL_PHP_ATTRIBUTE_ARRAY_LENGTH_LIMIT",
        "OTEL_BSP_MAX_QUEUE_SIZE",
        "OTEL_BSP_MAX_EXPORT_BATCH_SIZE",
        "OTEL_BSP_SCHEDULE_DELAY",
        "OTEL_BSP_MAX_CONCURRENT_EXPORTS",
        "OTEL_PHP_SHUTDOWN_TIMEOUT",
        "OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS",
        "OTEL_PHP_EXPORT_RETRY_MAX_ELAPSED",
        "OTEL_PHP_EXPORT_RETRY_INITIAL_BACKOFF",
    ] {
        name.hash(&mut hasher);
        env::var(name).unwrap_or_default().hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// True for the shared no-op provider used when the SDK or module is disabled
/// or a provider could not be built. Tracers from it skip span construction.
pub fn is_noop_provider(provider: &Arc<SdkTracerProvider>) -> bool {
    Arc::ptr_eq(provider, &NOOP_TRACER_PROVIDER)
}

fn no_op_entry() -> ProviderEntry {
    ProviderEntry {
        provider: NOOP_TRACER_PROVIDER.clone(),
        metrics: BatchMetrics::default(),
        shutdown_timeout: std::time::Duration::ZERO,
    }
}

fn add_exporter<E>(
    builder: TracerProviderBuilder,
    exporter: E,
    use_simple_exporter: bool,
    batch_config: Option<&BatchProcessorConfig>,
    metrics: &BatchMetrics,
) -> Result<TracerProviderBuilder, String>
where
    E: SpanExporter + 'static,
{
    if use_simple_exporter {
        return Ok(builder.with_simple_exporter(exporter));
    }

    let config = batch_config
        .cloned()
        .ok_or_else(|| "bounded batch configuration is missing".to_string())?;
    let processor = BoundedBatchSpanProcessor::new(exporter, config, metrics.clone())?;
    Ok(builder.with_span_processor(processor))
}

fn build_otlp_exporter(
    settings: &TransportSettings,
    timeout: std::time::Duration,
) -> Result<OtlpSpanExporter, String> {
    match settings.protocol {
        otlp_transport::Protocol::HttpProtobuf => {
            tracing::debug!("Using http/protobuf trace exporter");
            let client = otlp_transport::build_http_client(settings, timeout)?;
            let mut builder = OtlpSpanExporter::builder()
                .with_http()
                .with_http_client(client)
                .with_protocol(Protocol::HttpBinary)
                .with_timeout(timeout);
            if settings.compression == otlp_transport::Compression::Gzip {
                builder = builder.with_compression(Compression::Gzip);
            }
            otlp_transport::with_exporter_env_masked(|| builder.build())
                .map_err(|error| format!("Failed to create OTLP HTTP trace exporter: {error:?}"))
        }
        otlp_transport::Protocol::Grpc => {
            tracing::debug!("Using gRPC trace exporter with tokio runtime");
            let runtime = init_tokio_runtime().map_err(|error| {
                format!("Failed to create runtime for OTLP gRPC trace exporter: {error:?}")
            })?;
            let mut builder = OtlpSpanExporter::builder()
                .with_tonic()
                .with_endpoint(settings.endpoint.as_str())
                .with_timeout(timeout)
                .with_metadata(otlp_transport::grpc_metadata(&settings.headers));
            if let Some(tls) = &settings.tls {
                builder = builder.with_tls_config(otlp_transport::grpc_tls_config(tls));
            }
            if settings.compression == otlp_transport::Compression::Gzip {
                builder = builder.with_compression(Compression::Gzip);
            }
            otlp_transport::with_exporter_env_masked(|| runtime.block_on(async { builder.build() }))
                .map_err(|error| format!("Failed to create OTLP gRPC trace exporter: {error:?}"))
        }
    }
}

pub fn init_once() {
    let key = get_tracer_provider_key();
    let mut providers = TRACER_PROVIDERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if providers.contains_key(&key) {
        tracing::debug!("tracer provider already exists for pid {}", key.0);
        return;
    }
    tracing::debug!("creating tracer provider for pid {}", key.0);
    let exporter_name = env::var("OTEL_TRACES_EXPORTER").unwrap_or_else(|_| "otlp".to_string());
    let simple_requested = env::var("OTEL_SPAN_PROCESSOR").as_deref() == Ok("simple");
    let use_simple_exporter =
        simple_requested && matches!(exporter_name.as_str(), "memory" | "console");
    if simple_requested && !use_simple_exporter {
        tracing::warn!(
            "Ignoring OTEL_SPAN_PROCESSOR=simple for network trace exporter; using bounded batch processing"
        );
    }
    tracing::debug!(
        "SpanProcessor={}",
        if use_simple_exporter {
            "simple"
        } else {
            "batch"
        }
    );
    if exporter_name == "none" {
        tracing::debug!("Using no-op trace exporter");
        let provider = SdkTracerProvider::builder()
            .with_resource(Resource::builder_empty().build())
            .with_sampler(AlwaysOff)
            .with_span_limits(util::trace_span_limits())
            .build();
        providers.insert(
            key,
            ProviderEntry {
                provider: Arc::new(provider),
                metrics: BatchMetrics::default(),
                shutdown_timeout: std::time::Duration::ZERO,
            },
        );
        return;
    }

    let batch_config = if use_simple_exporter {
        None
    } else {
        match BatchProcessorConfig::from_env() {
            Ok(config) => Some(config),
            Err(error) => {
                tracing::warn!(
                    "Invalid bounded batch configuration: {}; using a no-op trace provider",
                    error
                );
                providers.insert(key, no_op_entry());
                return;
            }
        }
    };
    let metrics = BatchMetrics::default();

    let resource = Resource::builder()
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
        .build();

    let mut builder = SdkTracerProvider::builder().with_span_limits(util::trace_span_limits());
    if exporter_name == "console" {
        tracing::debug!("Using Console trace exporter");
        let exporter = StdoutSpanExporter::default();
        builder = match add_exporter(
            builder,
            exporter,
            use_simple_exporter,
            batch_config.as_ref(),
            &metrics,
        ) {
            Ok(builder) => builder,
            Err(error) => {
                tracing::warn!(
                    "Failed to create trace processor: {}; using a no-op provider",
                    error
                );
                providers.insert(key, no_op_entry());
                return;
            }
        };
    } else if exporter_name == "memory" {
        tracing::debug!("Using in-memory test exporter");
        let exporter = MEMORY_EXPORTER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        builder = match add_exporter(
            builder,
            exporter,
            use_simple_exporter,
            batch_config.as_ref(),
            &metrics,
        ) {
            Ok(builder) => builder,
            Err(error) => {
                tracing::warn!(
                    "Failed to create trace processor: {}; using a no-op provider",
                    error
                );
                providers.insert(key, no_op_entry());
                return;
            }
        };
    } else {
        let Some(network_batch_config) = batch_config.as_ref() else {
            tracing::warn!(
                "Network trace exporter requires bounded batch configuration; using a no-op provider"
            );
            providers.insert(key, no_op_entry());
            return;
        };
        let protocol = env::var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL")
            .or_else(|_| env::var("OTEL_EXPORTER_OTLP_PROTOCOL"))
            .unwrap_or_else(|_| "grpc".to_string())
            .to_ascii_lowercase();
        let protocol = match protocol.as_str() {
            "grpc" => otlp_transport::Protocol::Grpc,
            "http/protobuf" => otlp_transport::Protocol::HttpProtobuf,
            _ => {
                tracing::warn!(
                    "Unsupported OTLP trace protocol {:?}; using a no-op trace provider",
                    protocol
                );
                providers.insert(key, no_op_entry());
                return;
            }
        };
        let settings = match TransportSettings::from_env(protocol) {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!("{}; using a no-op trace provider", error);
                providers.insert(key, no_op_entry());
                return;
            }
        };
        let exporter = match build_otlp_exporter(&settings, network_batch_config.export_timeout) {
            Ok(exporter) => exporter,
            Err(error) => {
                tracing::warn!("{}; using a no-op trace provider", error);
                providers.insert(key, no_op_entry());
                return;
            }
        };
        builder = match add_exporter(builder, exporter, false, batch_config.as_ref(), &metrics) {
            Ok(builder) => builder,
            Err(error) => {
                tracing::warn!(
                    "Failed to create trace processor: {}; using a no-op provider",
                    error
                );
                providers.insert(key, no_op_entry());
                return;
            }
        };
    }
    let provider = Arc::new(builder.with_resource(resource).build());
    providers.insert(
        key,
        ProviderEntry {
            provider,
            metrics,
            shutdown_timeout: batch_config
                .map(|config| config.shutdown_timeout)
                .unwrap_or(std::time::Duration::ZERO),
        },
    );
}

pub fn get_tracer_provider() -> Arc<SdkTracerProvider> {
    if module::is_disabled() || request::is_disabled() {
        tracing::debug!(
            "OpenTelemetry is disabled for this request, returning no-op tracer provider"
        );
        return NOOP_TRACER_PROVIDER.clone();
    }
    let key = get_tracer_provider_key();
    {
        let providers = TRACER_PROVIDERS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = providers.get(&key) {
            return entry.provider.clone();
        }
    }

    // A PID change means the process forked after request initialization. Build
    // a fresh provider/runtime lazily in the child rather than reusing parent
    // threads, sockets, or provider state.
    init_once();
    let providers = TRACER_PROVIDERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(entry) = providers.get(&key) {
        entry.provider.clone()
    } else {
        tracing::warn!(
            "no tracer provider initialized for pid {}, using no-op",
            key.0
        );
        NOOP_TRACER_PROVIDER.clone()
    }
}

pub fn force_flush() {
    let pid = process::id();
    let providers = TRACER_PROVIDERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let key = get_tracer_provider_key();
    if let Some(entry) = providers.get(&key) {
        tracing::info!("Flushing TracerProvider for pid {}", pid);
        match entry.provider.force_flush() {
            Ok(_) => tracing::debug!("OpenTelemetry tracer provider flush success"),
            Err(err) => tracing::warn!("Failed to flush OpenTelemetry tracer provider: {:?}", err),
        }
    } else {
        tracing::info!("no tracer provider to flush for pid {}", pid);
    }
}

fn get_runtime_metrics() -> BatchMetricsSnapshot {
    let providers = TRACER_PROVIDERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    providers
        .get(&get_tracer_provider_key())
        .map(|entry| entry.metrics.snapshot())
        .unwrap_or_default()
}

pub fn shutdown() {
    let pid = process::id();
    let entries: Vec<_> = {
        let mut providers = TRACER_PROVIDERS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let keys_to_remove: Vec<_> = providers
            .keys()
            .filter(|(k_pid, _)| *k_pid == pid)
            .cloned()
            .collect();
        keys_to_remove
            .into_iter()
            .filter_map(|key| providers.remove(&key))
            .collect()
    };
    if !entries.is_empty() {
        tracing::info!("Shutting down all TracerProviders for pid {}", pid);
        for entry in entries {
            let result = entry.provider.shutdown_with_timeout(entry.shutdown_timeout);
            let metrics_after = entry.metrics.snapshot();
            tracing::info!(
                "TraceProviderShutdown pid={} result={:?} sampled_ended={} queued={} exported={} dropped_queue_full={} dropped_export_failure={} dropped_shutdown={} export_failures={} export_retries={} export_retry_recovered={} queue_depth={} queue_high_watermark={}",
                pid,
                result,
                metrics_after.sampled_ended,
                metrics_after.queued,
                metrics_after.exported,
                metrics_after.dropped_queue_full,
                metrics_after.dropped_export_failure,
                metrics_after.dropped_shutdown,
                metrics_after.export_failures,
                metrics_after.export_retries,
                metrics_after.export_retry_recovered,
                metrics_after.queue_depth,
                metrics_after.queue_high_watermark,
            );
        }
    } else {
        tracing::info!("no tracer providers to shutdown for pid {}", pid);
    }
}

pub fn make_tracer_provider_class(
    tracer_class: TracerClass,
    tracer_provider_interface: Interface,
) -> ClassEntity<()> {
    let mut class =
        ClassEntity::<()>::new_with_default_state_constructor(TRACER_PROVIDER_CLASS_NAME);

    class.implements(tracer_provider_interface);
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method("getTracer", Visibility::Public, move |_this, arguments| {
            let provider = get_tracer_provider();
            let name = util::arg(arguments, 0)?.expect_z_str()?.to_str()?;
            let mut object = tracer_class.init_object()?;
            if is_noop_provider(&provider) {
                *object.as_mut_state() = None;
                return Ok::<_, phper::Error>(object);
            }
            let name = util::intern(name);

            let version = arguments
                .get(1)
                .and_then(|arg| arg.as_z_str())
                .and_then(|s| s.to_str().ok())
                .map(util::intern);

            let schema_url = arguments
                .get(2)
                .and_then(|arg| arg.as_z_str())
                .and_then(|s| s.to_str().ok())
                .map(util::intern);

            let attributes = arguments
                .get(3)
                .map(util::zval_iterable_to_array)
                .transpose()?;

            let mut scope_builder = InstrumentationScope::builder(name);
            if let Some(version) = version {
                scope_builder = scope_builder.with_version(version);
            }
            if let Some(schema_url) = schema_url {
                scope_builder = scope_builder.with_schema_url(schema_url);
            }
            if let Some(attributes) = attributes {
                scope_builder = scope_builder.with_attributes(util::zval_arr_to_key_value_vec(
                    attributes.expect_z_arr()?,
                    util::AttributeDestination::Scope,
                ));
            }
            let scope = scope_builder.build();

            *object.as_mut_state() = Some(provider.tracer_with_scope(scope));
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
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(String::from(
            r"OpenTelemetry\API\Trace\TracerInterface",
        ))));

    class.add_method("forceFlush", Visibility::Public, |_, _| {
        force_flush();
        Ok::<_, Infallible>(())
    });

    class
        .add_method("getRuntimeMetrics", Visibility::Public, |_, _| {
            let metrics = get_runtime_metrics();
            let mut result = ZArray::new();
            result.insert("sampled_started", metrics.sampled_started as i64);
            result.insert("sampled_ended", metrics.sampled_ended as i64);
            result.insert("queued", metrics.queued as i64);
            result.insert("exported", metrics.exported as i64);
            result.insert("dropped_queue_full", metrics.dropped_queue_full as i64);
            result.insert(
                "dropped_export_failure",
                metrics.dropped_export_failure as i64,
            );
            result.insert("dropped_shutdown", metrics.dropped_shutdown as i64);
            result.insert("export_failures", metrics.export_failures as i64);
            result.insert("export_retries", metrics.export_retries as i64);
            result.insert(
                "export_retry_recovered",
                metrics.export_retry_recovered as i64,
            );
            result.insert("queue_depth", metrics.queue_depth as i64);
            result.insert("queue_high_watermark", metrics.queue_high_watermark as i64);
            result.insert("in_flight", metrics.in_flight as i64);
            Ok::<_, Infallible>(result)
        })
        .return_type(ReturnType::new(ReturnTypeHint::Array));

    class
}
