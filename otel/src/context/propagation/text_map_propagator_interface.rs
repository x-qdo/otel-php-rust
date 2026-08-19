use phper::{
    classes::{Interface, InterfaceEntity},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};

pub const GETTER_INTERFACE: &str = r"OpenTelemetry\Context\Propagation\PropagationGetterInterface";
pub const EXTENDED_GETTER_INTERFACE: &str =
    r"OpenTelemetry\Context\Propagation\ExtendedPropagationGetterInterface";
pub const SETTER_INTERFACE: &str = r"OpenTelemetry\Context\Propagation\PropagationSetterInterface";
pub const TEXT_MAP_INTERFACE: &str =
    r"OpenTelemetry\Context\Propagation\TextMapPropagatorInterface";
pub const RESPONSE_INTERFACE: &str =
    r"OpenTelemetry\Context\Propagation\ResponsePropagatorInterface";
const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";

pub fn make_propagation_getter_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(GETTER_INTERFACE);
    interface
        .add_method("keys")
        .argument(Argument::new("carrier"))
        .return_type(ReturnType::new(ReturnTypeHint::Array));
    interface
        .add_method("get")
        .argument(Argument::new("carrier"))
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::String).allow_null());
    interface
}

pub fn make_extended_propagation_getter_interface(getter: Interface) -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(EXTENDED_GETTER_INTERFACE);
    interface.extends(getter);
    interface
        .add_method("getAll")
        .argument(Argument::new("carrier"))
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::Array));
    interface
}

pub fn make_propagation_setter_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(SETTER_INTERFACE);
    interface
        .add_method("set")
        .argument(Argument::new("carrier").by_ref())
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .argument(Argument::new("value").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::Void));
    interface
}

fn inject_method(interface: &mut InterfaceEntity) {
    interface
        .add_method("inject")
        .argument(
            Argument::new("carrier")
                .with_type_hint(ArgumentTypeHint::Mixed)
                .by_ref(),
        )
        .argument(
            Argument::new("setter")
                .with_type_hint(ArgumentTypeHint::ClassEntry(SETTER_INTERFACE.to_string()))
                .allow_null()
                .with_default_value("NULL"),
        )
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string()))
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));
}

pub fn make_text_map_propagator_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(TEXT_MAP_INTERFACE);
    interface
        .add_method("fields")
        .return_type(ReturnType::new(ReturnTypeHint::Array));
    inject_method(&mut interface);
    interface
        .add_method("extract")
        .argument(Argument::new("carrier"))
        .argument(
            Argument::new("getter")
                .with_type_hint(ArgumentTypeHint::ClassEntry(GETTER_INTERFACE.to_string()))
                .allow_null()
                .with_default_value("NULL"),
        )
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string()))
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_INTERFACE.to_string(),
        )));
    interface
}

pub fn make_response_propagator_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(RESPONSE_INTERFACE);
    inject_method(&mut interface);
    interface
}
