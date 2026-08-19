use crate::context::context_key::CONTEXT_KEY_INTERFACE;
use phper::{
    classes::InterfaceEntity,
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};

const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";
const IMPLICIT_INTERFACE: &str = r"OpenTelemetry\Context\ImplicitContextKeyedInterface";

pub fn make_context_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(CONTEXT_INTERFACE);
    interface
        .add_static_method("createKey")
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_KEY_INTERFACE.to_string(),
        )));
    interface
        .add_static_method("getCurrent")
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_INTERFACE.to_string(),
        )));
    interface
        .add_method("activate")
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\Context\ScopeInterface".to_string(),
        )));
    interface
        .add_method("with")
        .argument(
            Argument::new("key").with_type_hint(ArgumentTypeHint::ClassEntry(
                CONTEXT_KEY_INTERFACE.to_string(),
            )),
        )
        .argument(Argument::new("value"))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_INTERFACE.to_string(),
        )));
    interface
        .add_method("withContextValue")
        .argument(
            Argument::new("value")
                .with_type_hint(ArgumentTypeHint::ClassEntry(IMPLICIT_INTERFACE.to_string())),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_INTERFACE.to_string(),
        )));
    interface
        .add_method("get")
        .argument(
            Argument::new("key").with_type_hint(ArgumentTypeHint::ClassEntry(
                CONTEXT_KEY_INTERFACE.to_string(),
            )),
        );
    interface
}

pub fn make_implicit_context_keyed_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(IMPLICIT_INTERFACE);
    interface
        .add_method("activate")
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\Context\ScopeInterface".to_string(),
        )));
    interface
        .add_method("storeInContext")
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string())),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_INTERFACE.to_string(),
        )));
    interface
}
