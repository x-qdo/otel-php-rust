use phper::classes::InterfaceEntity;

pub fn make_signals_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(r"OpenTelemetry\API\Signals");
    interface.add_constant("TRACE", "trace");
    interface.add_constant("METRICS", "metrics");
    interface.add_constant("LOGS", "logs");
    interface
}
