use crate::{
    context::context::ContextClass,
    logs::{
        log_record::{
            LogRecordState, any_value, nanos_to_system_time, set_attributes, set_context,
        },
        logger::{LoggerState, emit_state},
        severity::{otel_severity, severity_number},
    },
};
use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};
use std::convert::Infallible;

pub const EVENT_LOGGER_CLASS_NAME: &str = r"OpenTelemetry\API\Logs\EventLogger";
pub type EventLoggerClass = StateClass<LoggerState>;

pub fn make_event_logger_class(
    interface: Interface,
    context_class: ContextClass,
) -> ClassEntity<LoggerState> {
    let mut class: ClassEntity<LoggerState> =
        ClassEntity::new_with_default_state_constructor(EVENT_LOGGER_CLASS_NAME);
    class.set_final();
    class.implements(interface);
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method("emit", Visibility::Public, move |this, arguments| {
            let logger_state = this.as_state();
            if !logger_state.enabled {
                return Ok::<_, phper::Error>(());
            }
            let Some(logger) = logger_state.logger.clone() else {
                return Ok(());
            };

            let mut record = LogRecordState {
                event_name: Some(
                    crate::util::arg(arguments, 0)?
                        .expect_z_str()?
                        .to_str()?
                        .to_string(),
                ),
                ..LogRecordState::default()
            };
            if let Some(body) = arguments.get(1) {
                record.body = any_value(body);
            }
            if let Some(timestamp) = arguments.get(2).and_then(|value| value.as_long()) {
                record.timestamp = Some(nanos_to_system_time(timestamp));
            }
            if let Some(context) = arguments.get(3) {
                set_context(&mut record, context, &context_class)?;
            }
            if let Some(severity) = arguments
                .get(4)
                .filter(|value| !value.get_type_info().is_null())
            {
                let number = severity_number(severity)?;
                record.severity = otel_severity(number);
            }
            if let Some(attributes) = arguments.get(5) {
                set_attributes(&mut record, attributes)?;
            }
            emit_state(&logger, &record);
            Ok(())
        })
        .argument(Argument::new("name").with_type_hint(ArgumentTypeHint::String))
        .argument(
            Argument::new("body")
                .with_type_hint(ArgumentTypeHint::Mixed)
                .with_default_value("NULL"),
        )
        .argument(
            Argument::new("timestamp")
                .with_type_hint(ArgumentTypeHint::Int)
                .allow_null()
                .with_default_value("NULL"),
        )
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
                .with_type_hint(ArgumentTypeHint::ClassEntry(
                    r"OpenTelemetry\API\Logs\Severity".to_string(),
                ))
                .allow_null()
                .with_default_value("NULL"),
        )
        .argument(
            Argument::new("attributes")
                .with_type_hint(ArgumentTypeHint::Iterable)
                .with_default_value("[]"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    class
}
