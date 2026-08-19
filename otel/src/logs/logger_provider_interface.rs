use phper::{
    classes::InterfaceEntity,
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};

fn nullable_string(name: &str) -> Argument {
    Argument::new(name)
        .with_type_hint(ArgumentTypeHint::String)
        .allow_null()
        .with_default_value("NULL")
}

fn add_provider_method(interface: &mut InterfaceEntity, method: &str, result: &str) {
    interface
        .add_method(method)
        .argument(Argument::new("name").with_type_hint(ArgumentTypeHint::String))
        .argument(nullable_string("version"))
        .argument(nullable_string("schemaUrl"))
        .argument(
            Argument::new("attributes")
                .with_type_hint(ArgumentTypeHint::Iterable)
                .with_default_value("[]"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            result.to_string(),
        )));
}

pub fn make_logger_provider_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(r"OpenTelemetry\API\Logs\LoggerProviderInterface");
    add_provider_method(
        &mut interface,
        "getLogger",
        r"OpenTelemetry\API\Logs\LoggerInterface",
    );
    interface
}

pub fn make_event_logger_provider_interface() -> InterfaceEntity {
    let mut interface =
        InterfaceEntity::new(r"OpenTelemetry\API\Logs\EventLoggerProviderInterface");
    add_provider_method(
        &mut interface,
        "getEventLogger",
        r"OpenTelemetry\API\Logs\EventLoggerInterface",
    );
    interface
}
