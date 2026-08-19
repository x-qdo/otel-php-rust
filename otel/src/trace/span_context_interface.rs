use phper::{
    classes::InterfaceEntity,
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};

pub const SPAN_CONTEXT_INTERFACE: &str = r"OpenTelemetry\API\Trace\SpanContextInterface";
pub const TRACE_STATE_INTERFACE: &str = r"OpenTelemetry\API\Trace\TraceStateInterface";

fn add_factory(interface: &mut InterfaceEntity, name: &str) {
    interface
        .add_static_method(name)
        .argument(Argument::new("traceId").with_type_hint(ArgumentTypeHint::String))
        .argument(Argument::new("spanId").with_type_hint(ArgumentTypeHint::String))
        .argument(
            Argument::new("traceFlags")
                .with_type_hint(ArgumentTypeHint::Int)
                .with_default_value(r"\OpenTelemetry\API\Trace\TraceFlags::DEFAULT"),
        )
        .argument(
            Argument::new("traceState")
                .with_type_hint(ArgumentTypeHint::ClassEntry(TRACE_STATE_INTERFACE.to_string()))
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_CONTEXT_INTERFACE.to_string(),
        )));
}

pub fn make_span_context_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(SPAN_CONTEXT_INTERFACE);
    add_factory(&mut interface, "createFromRemoteParent");
    interface
        .add_static_method("getInvalid")
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_CONTEXT_INTERFACE.to_string(),
        )));
    add_factory(&mut interface, "create");

    for method in ["getTraceId", "getTraceIdBinary", "getSpanId", "getSpanIdBinary"] {
        interface
            .add_method(method)
            .return_type(ReturnType::new(ReturnTypeHint::String));
    }
    interface
        .add_method("getTraceFlags")
        .return_type(ReturnType::new(ReturnTypeHint::Int));
    interface
        .add_method("getTraceState")
        .return_type(
            ReturnType::new(ReturnTypeHint::ClassEntry(TRACE_STATE_INTERFACE.to_string()))
                .allow_null(),
        );
    for method in ["isValid", "isRemote", "isSampled"] {
        interface
            .add_method(method)
            .return_type(ReturnType::new(ReturnTypeHint::Bool));
    }
    interface
}
