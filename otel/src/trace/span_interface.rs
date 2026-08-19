use phper::{classes::InterfaceEntity, functions::ReturnType, types::ReturnTypeHint};

pub fn make_span_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(r"OpenTelemetry\API\Trace\SpanInterface");

    interface
        .add_method("isRecording")
        .return_type(ReturnType::new(ReturnTypeHint::Bool));

    interface
}
