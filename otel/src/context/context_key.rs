use phper::{
    classes::{ClassEntity, Interface, InterfaceEntity, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};
use std::convert::Infallible;

pub const CONTEXT_KEY_INTERFACE: &str = r"OpenTelemetry\Context\ContextKeyInterface";
pub const CONTEXT_KEY_CLASS: &str = r"OpenTelemetry\Context\ContextKey";
pub const CONTEXT_KEYS_CLASS: &str = r"OpenTelemetry\Context\ContextKeys";

pub type ContextKeyClass = StateClass<Option<String>>;
pub type ContextKeysClass = StateClass<()>;

pub fn get_or_create_context_key(
    keys_class: &ContextKeysClass,
    key_class: &ContextKeyClass,
    property: &str,
    name: &str,
) -> phper::Result<ZVal> {
    if let Some(value) = keys_class
        .as_class_entry()
        .get_static_property(property)
        .filter(|value| value.as_z_obj().is_some())
    {
        return Ok(value.clone());
    }
    let mut object = key_class.init_object()?;
    *object.as_mut_state() = Some(name.to_string());
    let value = ZVal::from(object);
    keys_class
        .as_class_entry()
        .set_static_property(property, value.clone());
    Ok(value)
}

pub fn make_context_key_interface() -> InterfaceEntity {
    InterfaceEntity::new(CONTEXT_KEY_INTERFACE)
}

pub fn make_context_key_class(interface: Interface) -> ClassEntity<Option<String>> {
    let mut class = ClassEntity::new_with_default_state_constructor(CONTEXT_KEY_CLASS);
    class.set_final();
    class.implements(interface);
    class
        .add_method("__construct", Visibility::Public, |this, arguments| {
            *this.as_mut_state() = arguments
                .first()
                .and_then(ZVal::as_z_str)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            Ok::<_, Infallible>(())
        })
        .argument(
            Argument::new("name")
                .with_type_hint(ArgumentTypeHint::String)
                .allow_null()
                .with_default_value("NULL"),
        );
    class
        .add_method("name", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.as_state().clone())
        })
        .return_type(ReturnType::new(ReturnTypeHint::String).allow_null());
    class
}

pub fn make_context_keys_class(key_class: ContextKeyClass) -> ClassEntity<()> {
    let mut class = ClassEntity::new_with_default_state_constructor(CONTEXT_KEYS_CLASS);
    class.set_final();
    class.add_static_property("span", Visibility::Private, ());
    class.add_static_property("baggage", Visibility::Private, ());
    let class_entry = class.bound_class();

    for (method, name) in [
        ("span", "opentelemetry-trace-span-key"),
        ("baggage", "opentelemetry-trace-baggage-key"),
    ] {
        let class_entry = class_entry.clone();
        let key_class = key_class.clone();
        class
            .add_static_method(method, Visibility::Public, move |_| {
                get_or_create_context_key(&class_entry, &key_class, method, name)
            })
            .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
                CONTEXT_KEY_INTERFACE.to_string(),
            )));
    }
    class
}
