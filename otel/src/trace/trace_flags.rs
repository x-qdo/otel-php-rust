use phper::classes::InterfaceEntity;

pub const TRACE_FLAGS_INTERFACE: &str = r"OpenTelemetry\API\Trace\TraceFlags";

pub fn make_trace_flags_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(TRACE_FLAGS_INTERFACE);
    interface.add_constant("SAMPLED", 0x01i64);
    interface.add_constant("RANDOM", 0x02i64);
    interface.add_constant("DEFAULT", 0x00i64);
    interface
}
