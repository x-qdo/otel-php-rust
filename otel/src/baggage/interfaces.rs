use phper::{
    classes::{Interface, InterfaceEntity},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};

pub const BAGGAGE_INTERFACE: &str = r"OpenTelemetry\API\Baggage\BaggageInterface";
pub const BAGGAGE_BUILDER_INTERFACE: &str = r"OpenTelemetry\API\Baggage\BaggageBuilderInterface";
pub const METADATA_INTERFACE: &str = r"OpenTelemetry\API\Baggage\MetadataInterface";
pub const ENTRY_CLASS: &str = r"OpenTelemetry\API\Baggage\Entry";
const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";

fn baggage_return() -> ReturnType {
    ReturnType::new(ReturnTypeHint::ClassEntry(BAGGAGE_INTERFACE.to_string()))
}

fn builder_return() -> ReturnType {
    ReturnType::new(ReturnTypeHint::ClassEntry(
        BAGGAGE_BUILDER_INTERFACE.to_string(),
    ))
}

pub fn make_metadata_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(METADATA_INTERFACE);
    interface
        .add_method("getValue")
        .return_type(ReturnType::new(ReturnTypeHint::String));
    interface
}

pub fn make_baggage_builder_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(BAGGAGE_BUILDER_INTERFACE);
    interface
        .add_method("set")
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .argument(Argument::new("value").with_type_hint(ArgumentTypeHint::Mixed))
        .argument(
            Argument::new("metadata")
                .with_type_hint(ArgumentTypeHint::ClassEntry(METADATA_INTERFACE.to_string()))
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(builder_return());
    interface
        .add_method("remove")
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(builder_return());
    interface.add_method("build").return_type(baggage_return());
    interface
}

pub fn make_baggage_interface(implicit_context_keyed: Interface) -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(BAGGAGE_INTERFACE);
    interface.extends(implicit_context_keyed);
    interface
        .add_static_method("fromContext")
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string())),
        )
        .return_type(baggage_return());
    interface
        .add_static_method("getBuilder")
        .return_type(builder_return());
    for method in ["getCurrent", "getEmpty"] {
        interface
            .add_static_method(method)
            .return_type(baggage_return());
    }
    interface
        .add_method("getEntry")
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(
            ReturnType::new(ReturnTypeHint::ClassEntry(ENTRY_CLASS.to_string())).allow_null(),
        );
    interface
        .add_method("getValue")
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String));
    interface
        .add_method("getAll")
        .return_type(ReturnType::new(ReturnTypeHint::Iterable));
    interface
        .add_method("isEmpty")
        .return_type(ReturnType::new(ReturnTypeHint::Bool));
    interface
        .add_method("toBuilder")
        .return_type(builder_return());
    interface
}
