use crate::trace::span_context_interface::SPAN_CONTEXT_INTERFACE;
use phper::{
    classes::{Interface, InterfaceEntity},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};

pub const SPAN_INTERFACE: &str = r"OpenTelemetry\API\Trace\SpanInterface";
const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";

fn span_return() -> ReturnType {
    ReturnType::new(ReturnTypeHint::ClassEntry(SPAN_INTERFACE.to_string()))
}

pub fn make_span_interface(implicit_context_keyed: Interface) -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(SPAN_INTERFACE);
    interface.extends(implicit_context_keyed);

    interface
        .add_static_method("fromContext")
        .argument(
            Argument::new("context").with_type_hint(ArgumentTypeHint::ClassEntry(
                CONTEXT_INTERFACE.to_string(),
            )),
        )
        .return_type(span_return());
    for method in ["getCurrent", "getInvalid"] {
        interface.add_static_method(method).return_type(span_return());
    }
    interface
        .add_static_method("wrap")
        .argument(
            Argument::new("spanContext").with_type_hint(ArgumentTypeHint::ClassEntry(
                SPAN_CONTEXT_INTERFACE.to_string(),
            )),
        )
        .return_type(span_return());

    interface
        .add_method("getContext")
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_CONTEXT_INTERFACE.to_string(),
        )));

    interface
        .add_method("isRecording")
        .return_type(ReturnType::new(ReturnTypeHint::Bool));

    interface
        .add_method("setAttribute")
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .argument(
            Argument::new("value").with_type_hint(ArgumentTypeHint::Union(vec![
                ArgumentTypeHint::Bool,
                ArgumentTypeHint::Int,
                ArgumentTypeHint::Float,
                ArgumentTypeHint::String,
                ArgumentTypeHint::Array,
                ArgumentTypeHint::Null,
            ])),
        )
        .return_type(span_return());
    interface
        .add_method("setAttributes")
        .argument(Argument::new("attributes").with_type_hint(ArgumentTypeHint::Iterable))
        .return_type(span_return());
    interface
        .add_method("addLink")
        .argument(
            Argument::new("context").with_type_hint(ArgumentTypeHint::ClassEntry(
                SPAN_CONTEXT_INTERFACE.to_string(),
            )),
        )
        .argument(
            Argument::new("attributes")
                .with_type_hint(ArgumentTypeHint::Iterable)
                .with_default_value("[]"),
        )
        .return_type(span_return());
    interface
        .add_method("addEvent")
        .argument(Argument::new("name").with_type_hint(ArgumentTypeHint::String))
        .argument(
            Argument::new("attributes")
                .with_type_hint(ArgumentTypeHint::Iterable)
                .with_default_value("[]"),
        )
        .argument(
            Argument::new("timestamp")
                .with_type_hint(ArgumentTypeHint::Int)
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(span_return());
    interface
        .add_method("recordException")
        .argument(
            Argument::new("exception")
                .with_type_hint(ArgumentTypeHint::ClassEntry("Throwable".to_string())),
        )
        .argument(
            Argument::new("attributes")
                .with_type_hint(ArgumentTypeHint::Iterable)
                .with_default_value("[]"),
        )
        .return_type(span_return());
    interface
        .add_method("updateName")
        .argument(Argument::new("name").with_type_hint(ArgumentTypeHint::String))
        .return_type(span_return());
    interface
        .add_method("setStatus")
        .argument(Argument::new("code").with_type_hint(ArgumentTypeHint::String))
        .argument(
            Argument::new("description")
                .with_type_hint(ArgumentTypeHint::String)
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(span_return());
    interface
        .add_method("end")
        .argument(
            Argument::new("endEpochNanos")
                .with_type_hint(ArgumentTypeHint::Int)
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    interface
}
