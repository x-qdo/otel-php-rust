use phper::{classes::InterfaceEntity, functions::ReturnType, types::ReturnTypeHint};

pub const DETACHED: i64 = i64::MIN;
pub const INACTIVE: i64 = 1_i64 << 62;
pub const MISMATCH: i64 = 1_i64 << 61;

pub fn make_scope_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(r"OpenTelemetry\Context\ScopeInterface");
    interface.add_constant("DETACHED", DETACHED);
    interface.add_constant("INACTIVE", INACTIVE);
    interface.add_constant("MISMATCH", MISMATCH);
    interface
        .add_method("detach")
        .return_type(ReturnType::new(ReturnTypeHint::Int));

    interface
        .add_method("context")
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(String::from(
            r"OpenTelemetry\Context\ContextInterface",
        ))));

    interface
}
