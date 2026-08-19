use phper::classes::InterfaceEntity;

pub const SPAN_KIND_INTERFACE: &str = r"OpenTelemetry\API\Trace\SpanKind";

pub fn make_span_kind_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(SPAN_KIND_INTERFACE);
    interface.add_constant("KIND_INTERNAL", 0i64);
    interface.add_constant("KIND_CLIENT", 1i64);
    interface.add_constant("KIND_SERVER", 2i64);
    interface.add_constant("KIND_PRODUCER", 3i64);
    interface.add_constant("KIND_CONSUMER", 4i64);
    interface
}
