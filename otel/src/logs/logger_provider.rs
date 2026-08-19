use crate::{
    logs::{
        event_logger::EventLoggerClass,
        logger::{LoggerClass, LoggerState},
        memory_exporter::MEMORY_EXPORTER,
    },
    request,
    runtime::init_tokio_runtime,
    util,
};
use once_cell::sync::Lazy;
use opentelemetry::{InstrumentationScope, KeyValue, logs::LoggerProvider};
use opentelemetry_otlp::{LogExporter as OtlpLogExporter, Protocol, WithExportConfig};
use opentelemetry_sdk::{
    Resource,
    logs::{BatchConfigBuilder, BatchLogProcessor, SdkLoggerProvider, SimpleLogProcessor},
};
use opentelemetry_stdout::LogExporter as StdoutLogExporter;
use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};
use std::{
    cell::RefCell,
    collections::{HashMap, hash_map::DefaultHasher},
    convert::Infallible,
    env,
    hash::{Hash, Hasher},
    process,
    sync::{Arc, Mutex},
    time::Duration,
};

pub const LOGGER_PROVIDER_CLASS_NAME: &str = r"OpenTelemetry\API\Logs\LoggerProvider";
pub type LoggerProviderClass = StateClass<()>;
pub type ProviderKey = (u32, String);

pub struct NativeLoggerProvider {
    pub sdk: SdkLoggerProvider,
    pub enabled: bool,
    pub key: ProviderKey,
    shutdown_timeout: Duration,
}

static LOGGER_PROVIDERS: Lazy<Mutex<HashMap<ProviderKey, Arc<NativeLoggerProvider>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

thread_local! {
    static REQUEST_CONFIG_HASH: RefCell<Option<String>> = const { RefCell::new(None) };
}

static NOOP_LOGGER_PROVIDER: Lazy<Arc<NativeLoggerProvider>> = Lazy::new(|| {
    Arc::new(NativeLoggerProvider {
        sdk: SdkLoggerProvider::builder()
            .with_resource(Resource::builder_empty().build())
            .build(),
        enabled: false,
        key: (process::id(), "noop".to_string()),
        shutdown_timeout: Duration::ZERO,
    })
});

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
        "OTEL_LOGS_EXPORTER",
        "OTEL_LOGS_PROCESSOR",
        "OTEL_EXPORTER_OTLP_PROTOCOL",
        "OTEL_EXPORTER_OTLP_LOGS_PROTOCOL",
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
        "OTEL_EXPORTER_OTLP_HEADERS",
        "OTEL_EXPORTER_OTLP_LOGS_HEADERS",
        "OTEL_EXPORTER_OTLP_COMPRESSION",
        "OTEL_EXPORTER_OTLP_LOGS_COMPRESSION",
        "OTEL_EXPORTER_OTLP_TIMEOUT",
        "OTEL_EXPORTER_OTLP_LOGS_TIMEOUT",
        "OTEL_EXPORTER_OTLP_CERTIFICATE",
        "OTEL_EXPORTER_OTLP_LOGS_CERTIFICATE",
        "OTEL_EXPORTER_OTLP_CLIENT_KEY",
        "OTEL_EXPORTER_OTLP_LOGS_CLIENT_KEY",
        "OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE",
        "OTEL_EXPORTER_OTLP_LOGS_CLIENT_CERTIFICATE",
        "OTEL_EXPORTER_OTLP_INSECURE",
        "OTEL_EXPORTER_OTLP_LOGS_INSECURE",
        "OTEL_BLRP_SCHEDULE_DELAY",
        "OTEL_BLRP_MAX_QUEUE_SIZE",
        "OTEL_BLRP_MAX_EXPORT_BATCH_SIZE",
        "OTEL_BLRP_EXPORT_TIMEOUT",
        "OTEL_PHP_SHUTDOWN_TIMEOUT",
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

fn shutdown_timeout() -> Duration {
    let millis = env::var("OTEL_PHP_SHUTDOWN_TIMEOUT")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(5_000)
        .clamp(1, 60_000);
    Duration::from_millis(millis)
}

fn with_processor<E: opentelemetry_sdk::logs::LogExporter + 'static>(
    builder: opentelemetry_sdk::logs::LoggerProviderBuilder,
    exporter: E,
    simple: bool,
) -> opentelemetry_sdk::logs::LoggerProviderBuilder {
    if simple {
        builder.with_log_processor(SimpleLogProcessor::new(exporter))
    } else {
        let processor = BatchLogProcessor::builder(exporter)
            .with_batch_config(BatchConfigBuilder::default().build())
            .build();
        builder.with_log_processor(processor)
    }
}

fn build_provider(key: ProviderKey) -> Arc<NativeLoggerProvider> {
    let exporter_name = env::var("OTEL_LOGS_EXPORTER").unwrap_or_else(|_| "otlp".to_string());
    let simple_requested = env::var("OTEL_LOGS_PROCESSOR").as_deref() == Ok("simple");
    let simple = simple_requested && matches!(exporter_name.as_str(), "memory" | "console");
    if simple_requested && !simple {
        tracing::warn!(
            "ignoring OTEL_LOGS_PROCESSOR=simple for a network exporter; using bounded batching"
        );
    }
    let timeout = shutdown_timeout();
    let builder = SdkLoggerProvider::builder().with_resource(resource());

    let (sdk, enabled) = match exporter_name.as_str() {
        "none" => (builder.build(), false),
        "console" => (
            with_processor(builder, StdoutLogExporter::default(), simple).build(),
            true,
        ),
        "memory" => {
            let exporter = MEMORY_EXPORTER
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            (with_processor(builder, exporter, simple).build(), true)
        }
        "otlp" => {
            let protocol = env::var("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL")
                .or_else(|_| env::var("OTEL_EXPORTER_OTLP_PROTOCOL"))
                .unwrap_or_else(|_| "grpc".to_string())
                .to_ascii_lowercase();
            let exporter = match protocol.as_str() {
                "http/protobuf" => OtlpLogExporter::builder()
                    .with_http()
                    .with_protocol(Protocol::HttpBinary)
                    .build(),
                "grpc" => match init_tokio_runtime() {
                    Ok(runtime) => {
                        runtime.block_on(async { OtlpLogExporter::builder().with_tonic().build() })
                    }
                    Err(error) => {
                        tracing::warn!("failed to create logs export runtime: {error}");
                        return Arc::new(NativeLoggerProvider {
                            sdk: builder.build(),
                            enabled: false,
                            key,
                            shutdown_timeout: Duration::ZERO,
                        });
                    }
                },
                other => {
                    tracing::warn!(
                        "unsupported OTLP logs protocol {other:?}; using no-op logs provider"
                    );
                    return Arc::new(NativeLoggerProvider {
                        sdk: builder.build(),
                        enabled: false,
                        key,
                        shutdown_timeout: Duration::ZERO,
                    });
                }
            };
            match exporter {
                Ok(exporter) => (with_processor(builder, exporter, false).build(), true),
                Err(error) => {
                    tracing::warn!(
                        "failed to create OTLP logs exporter: {error:?}; using no-op logs provider"
                    );
                    (builder.build(), false)
                }
            }
        }
        other => {
            tracing::warn!("unsupported OTEL_LOGS_EXPORTER={other:?}; using no-op logs provider");
            (builder.build(), false)
        }
    };

    Arc::new(NativeLoggerProvider {
        sdk,
        enabled,
        key,
        shutdown_timeout: timeout,
    })
}

pub fn init_once() {
    let key = provider_key();
    let mut providers = LOGGER_PROVIDERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if providers.contains_key(&key) {
        return;
    }
    providers.insert(key.clone(), build_provider(key));
}

pub fn get_logger_provider() -> Arc<NativeLoggerProvider> {
    if request::is_disabled() {
        return NOOP_LOGGER_PROVIDER.clone();
    }
    let key = provider_key();
    LOGGER_PROVIDERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&key)
        .cloned()
        .unwrap_or_else(|| {
            tracing::warn!(
                "no logger provider initialized for pid {}; using no-op",
                key.0
            );
            NOOP_LOGGER_PROVIDER.clone()
        })
}

pub fn shutdown() {
    let pid = process::id();
    let providers = {
        let mut providers = LOGGER_PROVIDERS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let keys = providers
            .keys()
            .filter(|(provider_pid, _)| *provider_pid == pid)
            .cloned()
            .collect::<Vec<_>>();
        keys.into_iter()
            .filter_map(|key| providers.remove(&key))
            .collect::<Vec<_>>()
    };
    for provider in providers {
        if let Err(error) = provider
            .sdk
            .shutdown_with_timeout(provider.shutdown_timeout)
        {
            tracing::warn!("logs provider shutdown failed: {error}");
        }
    }
}

fn optional_string(arguments: &[ZVal], index: usize) -> Option<String> {
    arguments
        .get(index)
        .and_then(ZVal::as_z_str)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn logger_state(arguments: &[ZVal]) -> phper::Result<LoggerState> {
    let provider = get_logger_provider();
    let name = crate::util::arg(arguments, 0)?
        .expect_z_str()?
        .to_str()?
        .to_string();
    let mut scope = InstrumentationScope::builder(name);
    if let Some(version) = optional_string(arguments, 1) {
        scope = scope.with_version(version);
    }
    if let Some(schema_url) = optional_string(arguments, 2) {
        scope = scope.with_schema_url(schema_url);
    }
    if let Some(attributes) = arguments.get(3) {
        let attributes =
            util::zval_iterable_to_key_value_vec(attributes, util::AttributeDestination::Scope)?;
        if !attributes.is_empty() {
            scope = scope.with_attributes(attributes);
        }
    }
    Ok(LoggerState {
        logger: Some(provider.sdk.logger_with_scope(scope.build())),
        enabled: provider.enabled,
    })
}

fn provider_arguments() -> [Argument; 4] {
    [
        Argument::new("name").with_type_hint(ArgumentTypeHint::String),
        Argument::new("version")
            .with_type_hint(ArgumentTypeHint::String)
            .allow_null()
            .with_default_value("NULL"),
        Argument::new("schemaUrl")
            .with_type_hint(ArgumentTypeHint::String)
            .allow_null()
            .with_default_value("NULL"),
        Argument::new("attributes")
            .with_type_hint(ArgumentTypeHint::Iterable)
            .with_default_value("[]"),
    ]
}

pub fn make_logger_provider_class(
    logger_class: LoggerClass,
    event_logger_class: EventLoggerClass,
    logger_provider_interface: Interface,
    event_logger_provider_interface: Interface,
) -> ClassEntity<()> {
    let mut class = ClassEntity::new_with_default_state_constructor(LOGGER_PROVIDER_CLASS_NAME);
    class.set_final();
    class.implements(logger_provider_interface);
    class.implements(event_logger_provider_interface);
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method("getLogger", Visibility::Public, move |_, arguments| {
            let mut object = logger_class.init_object()?;
            *object.as_mut_state() = logger_state(arguments)?;
            Ok::<_, phper::Error>(object)
        })
        .arguments(provider_arguments())
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\API\Logs\LoggerInterface".to_string(),
        )));

    class
        .add_method("getEventLogger", Visibility::Public, move |_, arguments| {
            let mut object = event_logger_class.init_object()?;
            *object.as_mut_state() = logger_state(arguments)?;
            Ok::<_, phper::Error>(object)
        })
        .arguments(provider_arguments())
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\API\Logs\EventLoggerInterface".to_string(),
        )));

    class
}
