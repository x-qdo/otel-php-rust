use phper::{
    classes::{Interface, InterfaceEntity},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};

pub const STORAGE_INTERFACE: &str = r"OpenTelemetry\Context\ContextStorageInterface";
pub const STORAGE_SCOPE_INTERFACE: &str = r"OpenTelemetry\Context\ContextStorageScopeInterface";
pub const EXECUTION_CONTEXT_AWARE_INTERFACE: &str =
    r"OpenTelemetry\Context\ExecutionContextAwareInterface";

pub fn make_context_storage_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(STORAGE_INTERFACE);
    interface.add_method("scope").return_type(
        ReturnType::new(ReturnTypeHint::ClassEntry(
            STORAGE_SCOPE_INTERFACE.to_string(),
        ))
        .allow_null(),
    );
    interface
        .add_method("current")
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\Context\ContextInterface".to_string(),
        )));
    interface
        .add_method("attach")
        .argument(
            Argument::new("context").with_type_hint(ArgumentTypeHint::ClassEntry(
                r"OpenTelemetry\Context\ContextInterface".to_string(),
            )),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            STORAGE_SCOPE_INTERFACE.to_string(),
        )));
    interface
}

pub fn make_context_storage_scope_interface(scope: Interface) -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(STORAGE_SCOPE_INTERFACE);
    interface.extends(scope);
    interface.extends(Interface::from_name("ArrayAccess"));
    interface
        .add_method("context")
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\Context\ContextInterface".to_string(),
        )));
    interface
        .add_method("offsetSet")
        .argument(Argument::new("offset"))
        .argument(Argument::new("value"))
        .return_type(ReturnType::new(ReturnTypeHint::Void));
    interface
}

pub fn make_execution_context_aware_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(EXECUTION_CONTEXT_AWARE_INTERFACE);
    for method in ["fork", "switch", "destroy"] {
        interface
            .add_method(method)
            .argument(
                Argument::new("id").with_type_hint(ArgumentTypeHint::Union(vec![
                    ArgumentTypeHint::Int,
                    ArgumentTypeHint::String,
                ])),
            )
            .return_type(ReturnType::new(ReturnTypeHint::Void));
    }
    interface
}
