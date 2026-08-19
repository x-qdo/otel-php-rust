use crate::logs::{
    log_record::{LOG_RECORD_CLASS_NAME, LogRecordState, selected_trace_context},
    log_record_builder::{LogRecordBuilderClass, LogRecordBuilderState},
};
use once_cell::sync::Lazy;
use opentelemetry::logs::{LogRecord, Logger};
use opentelemetry_sdk::logs::SdkLogger;
use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};
use std::{
    collections::HashSet,
    convert::Infallible,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub const LOGGER_CLASS_NAME: &str = r"OpenTelemetry\API\Logs\Logger";
const MAX_INTERNED_LOG_STRINGS: usize = 4_096;
const CARDINALITY_FALLBACK: &str = "<otel-cardinality-limit>";

static LOG_STRINGS: Lazy<Mutex<HashSet<&'static str>>> = Lazy::new(|| Mutex::new(HashSet::new()));
static CARDINALITY_WARNING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Default)]
pub struct LoggerState {
    pub logger: Option<SdkLogger>,
    pub enabled: bool,
}

pub type LoggerClass = StateClass<LoggerState>;

/// The Rust logs trait currently requires `&'static str` for event/severity
/// text. Bound the process-lifetime interner so attacker-controlled event names
/// cannot become an unbounded worker leak.
fn intern_log_string(value: &str) -> &'static str {
    let mut values = LOG_STRINGS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = values.get(value) {
        return existing;
    }
    if values.len() >= MAX_INTERNED_LOG_STRINGS {
        if !CARDINALITY_WARNING.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                "log event/severity string cardinality exceeded {MAX_INTERNED_LOG_STRINGS}; new values use a bounded fallback"
            );
        }
        return CARDINALITY_FALLBACK;
    }
    let value = Box::leak(value.to_string().into_boxed_str());
    values.insert(value);
    value
}

pub fn emit_state(logger: &SdkLogger, state: &LogRecordState) {
    let mut record = logger.create_log_record();
    if let Some(severity) = state.severity {
        record.set_severity_number(severity);
    }
    if let Some(body) = &state.body {
        record.set_body(body.clone());
    }
    if let Some(severity_text) = &state.severity_text {
        record.set_severity_text(intern_log_string(severity_text));
    }
    if let Some(event_name) = &state.event_name {
        record.set_event_name(intern_log_string(event_name));
    }
    if let Some(timestamp) = state.timestamp {
        record.set_timestamp(timestamp);
    }
    if let Some(timestamp) = state.observed_timestamp {
        record.set_observed_timestamp(timestamp);
    }
    if let Some(context) = selected_trace_context(state.context) {
        record.set_trace_context(context.trace_id, context.span_id, Some(context.trace_flags));
    }
    for (key, value) in &state.attributes {
        record.add_attribute(key.clone(), value.clone());
    }
    logger.emit(record);
}

pub fn make_logger_class(
    logger_interface: Interface,
    builder_class: LogRecordBuilderClass,
) -> ClassEntity<LoggerState> {
    let mut class: ClassEntity<LoggerState> =
        ClassEntity::new_with_default_state_constructor(LOGGER_CLASS_NAME);
    class.set_final();
    class.implements(logger_interface);
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method("emit", Visibility::Public, |this, arguments| {
            let state = this.as_state();
            if !state.enabled {
                return Ok::<_, phper::Error>(());
            }
            let Some(logger) = &state.logger else {
                return Ok(());
            };
            let record = crate::util::arg(arguments, 0)?.expect_z_obj()?;
            let record = unsafe { record.as_state_obj::<LogRecordState>().as_state() };
            emit_state(logger, record);
            Ok(())
        })
        .argument(
            Argument::new("logRecord").with_type_hint(ArgumentTypeHint::ClassEntry(
                LOG_RECORD_CLASS_NAME.to_string(),
            )),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    class
        .add_method("logRecordBuilder", Visibility::Public, move |this, _| {
            let mut object = builder_class.init_object()?;
            *object.as_mut_state() = LogRecordBuilderState {
                logger: this.as_state().logger.clone(),
                enabled: this.as_state().enabled,
                record: LogRecordState::default(),
            };
            Ok::<_, phper::Error>(object)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\API\Logs\LogRecordBuilderInterface".to_string(),
        )));

    class
        .add_method("isEnabled", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.as_state().enabled)
        })
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(
                    r"OpenTelemetry\Context\ContextInterface".to_string(),
                ))
                .allow_null()
                .with_default_value("NULL"),
        )
        .argument(
            Argument::new("severityNumber")
                .with_type_hint(ArgumentTypeHint::Int)
                .allow_null()
                .with_default_value("NULL"),
        )
        .argument(
            Argument::new("eventName")
                .with_type_hint(ArgumentTypeHint::String)
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Bool));

    class
}
