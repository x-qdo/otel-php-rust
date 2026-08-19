use phper::{
    classes::InterfaceEntity,
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};

const LOG_RECORD_CLASS: &str = r"OpenTelemetry\API\Logs\LogRecord";
const BUILDER_INTERFACE: &str = r"OpenTelemetry\API\Logs\LogRecordBuilderInterface";
const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";
const SEVERITY_ENUM: &str = r"OpenTelemetry\API\Logs\Severity";

fn builder_return() -> ReturnType {
    ReturnType::new(ReturnTypeHint::ClassEntry(BUILDER_INTERFACE.to_string()))
}

pub fn make_logger_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(r"OpenTelemetry\API\Logs\LoggerInterface");
    interface
        .add_method("emit")
        .argument(
            Argument::new("logRecord")
                .with_type_hint(ArgumentTypeHint::ClassEntry(LOG_RECORD_CLASS.to_string())),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));
    interface
        .add_method("logRecordBuilder")
        .return_type(builder_return());
    interface
        .add_method("isEnabled")
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string()))
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
    interface
}

pub fn make_log_record_builder_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(r"OpenTelemetry\API\Logs\LogRecordBuilderInterface");
    interface
        .add_method("setTimestamp")
        .argument(Argument::new("timestamp").with_type_hint(ArgumentTypeHint::Int))
        .return_type(builder_return());
    interface
        .add_method("setObservedTimestamp")
        .argument(Argument::new("timestamp").with_type_hint(ArgumentTypeHint::Int))
        .return_type(builder_return());
    interface
        .add_method("setContext")
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::Union(vec![
                    ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string()),
                    ArgumentTypeHint::False,
                ]))
                .allow_null(),
        )
        .return_type(builder_return());
    interface
        .add_method("setSeverityNumber")
        .argument(
            Argument::new("severityNumber").with_type_hint(ArgumentTypeHint::Union(vec![
                ArgumentTypeHint::ClassEntry(SEVERITY_ENUM.to_string()),
                ArgumentTypeHint::Int,
            ])),
        )
        .return_type(builder_return());
    interface
        .add_method("setSeverityText")
        .argument(Argument::new("severityText").with_type_hint(ArgumentTypeHint::String))
        .return_type(builder_return());
    interface
        .add_method("setBody")
        .argument(Argument::new("body").with_type_hint(ArgumentTypeHint::Mixed))
        .return_type(builder_return());
    interface
        .add_method("setAttribute")
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .argument(Argument::new("value").with_type_hint(ArgumentTypeHint::Mixed))
        .return_type(builder_return());
    interface
        .add_method("setAttributes")
        .argument(Argument::new("attributes").with_type_hint(ArgumentTypeHint::Iterable))
        .return_type(builder_return());
    interface
        .add_method("setException")
        .argument(
            Argument::new("exception")
                .with_type_hint(ArgumentTypeHint::ClassEntry("Throwable".to_string())),
        )
        .return_type(builder_return());
    interface
        .add_method("setEventName")
        .argument(Argument::new("eventName").with_type_hint(ArgumentTypeHint::String))
        .return_type(builder_return());
    interface
        .add_method("emit")
        .return_type(ReturnType::new(ReturnTypeHint::Void));
    interface
}

pub fn make_event_logger_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(r"OpenTelemetry\API\Logs\EventLoggerInterface");
    interface
        .add_method("emit")
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
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string()))
                .allow_null()
                .with_default_value("NULL"),
        )
        .argument(
            Argument::new("severityNumber")
                .with_type_hint(ArgumentTypeHint::ClassEntry(SEVERITY_ENUM.to_string()))
                .allow_null()
                .with_default_value("NULL"),
        )
        .argument(
            Argument::new("attributes")
                .with_type_hint(ArgumentTypeHint::Iterable)
                .with_default_value("[]"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));
    interface
}
