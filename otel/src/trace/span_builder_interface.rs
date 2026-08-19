use crate::trace::span_context_interface::SPAN_CONTEXT_INTERFACE;
use phper::{
    classes::InterfaceEntity,
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};

pub const SPAN_BUILDER_INTERFACE: &str = r"OpenTelemetry\API\Trace\SpanBuilderInterface";
const SPAN_INTERFACE: &str = r"OpenTelemetry\API\Trace\SpanInterface";
const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";

fn builder_return() -> ReturnType {
    ReturnType::new(ReturnTypeHint::ClassEntry(SPAN_BUILDER_INTERFACE.to_string()))
}

pub fn make_span_builder_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(SPAN_BUILDER_INTERFACE);
    interface
        .add_method("setParent")
        .argument(
            Argument::new("context").with_type_hint(ArgumentTypeHint::Union(vec![
                ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string()),
                ArgumentTypeHint::False,
                ArgumentTypeHint::Null,
            ])),
        )
        .return_type(builder_return());
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
        .add_method("setStartTimestamp")
        .argument(
            Argument::new("timestampNanos").with_type_hint(ArgumentTypeHint::Int),
        )
        .return_type(builder_return());
    interface
        .add_method("setSpanKind")
        .argument(Argument::new("spanKind").with_type_hint(ArgumentTypeHint::Int))
        .return_type(builder_return());
    interface
        .add_method("startSpan")
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_INTERFACE.to_string(),
        )));
    interface
}
