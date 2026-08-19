use crate::{
    baggage::{
        baggage_builder::BAGGAGE_BUILDER_CLASS,
        interfaces::{BAGGAGE_BUILDER_INTERFACE, BAGGAGE_INTERFACE, ENTRY_CLASS},
    },
    context::{
        context_class::{ContextClass, current_context_value},
        context_key::{ContextKeyClass, ContextKeysClass, get_or_create_context_key},
    },
};
use phper::{
    alloc::ToRefOwned,
    arrays::{IterKey, ZArr, ZArray},
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    objects::ZObj,
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};

pub const BAGGAGE_CLASS: &str = r"OpenTelemetry\API\Baggage\Baggage";
const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";
const SCOPE_INTERFACE: &str = r"OpenTelemetry\Context\ScopeInterface";

pub type BaggageEntries = Vec<(Vec<u8>, ZVal)>;
pub type BaggageClass = StateClass<BaggageEntries>;

pub fn entries_from_array(entries: &ZArr) -> BaggageEntries {
    entries
        .iter()
        .map(|(key, value)| {
            let key = match key {
                IterKey::Index(index) => index.to_string().into_bytes(),
                IterKey::ZStr(key) => key.to_bytes().to_vec(),
            };
            (key, value.clone())
        })
        .collect()
}

pub fn entries_to_array(entries: &BaggageEntries) -> ZArray {
    let mut result = ZArray::new();
    for (key, entry) in entries {
        result.insert(key.as_slice(), entry.clone());
    }
    result
}

pub fn init_baggage_object(
    class: &BaggageClass,
    entries: BaggageEntries,
) -> phper::Result<phper::objects::StateObject<BaggageEntries>> {
    let mut object = class.init_object()?;
    *object.as_mut_state() = entries;
    Ok(object)
}

pub fn empty_baggage(class: &BaggageClass) -> phper::Result<ZVal> {
    if let Some(value) = class
        .as_class_entry()
        .get_static_property("emptyBaggage")
        .filter(|value| value.as_z_obj().is_some())
    {
        return Ok(value.clone());
    }
    let value = ZVal::from(init_baggage_object(class, Vec::new())?);
    class
        .as_class_entry()
        .set_static_property("emptyBaggage", value.clone());
    Ok(value)
}

pub fn baggage_from_context(
    context: &mut ZObj,
    baggage_class: &BaggageClass,
    key_class: &ContextKeyClass,
    keys_class: &ContextKeysClass,
) -> phper::Result<ZVal> {
    let key = get_or_create_context_key(
        keys_class,
        key_class,
        "baggage",
        "opentelemetry-trace-baggage-key",
    )?;
    let baggage = context.call("get", &mut [key])?;
    if !baggage.get_type_info().is_null() {
        return Ok(baggage);
    }
    empty_baggage(baggage_class)
}

pub fn make_baggage_class(
    interface: Interface,
    context_class: ContextClass,
    key_class: ContextKeyClass,
    keys_class: ContextKeysClass,
) -> ClassEntity<BaggageEntries> {
    let mut class = ClassEntity::new_with_default_state_constructor(BAGGAGE_CLASS);
    class.set_final();
    class.state_cloner(Clone::clone);
    class.implements(interface);
    class.add_static_property("emptyBaggage", Visibility::Private, ());
    let baggage_class = class.bound_class();

    let from_baggage_class = baggage_class.clone();
    let from_key_class = key_class.clone();
    let from_keys_class = keys_class.clone();
    class
        .add_static_method("fromContext", Visibility::Public, move |arguments| {
            baggage_from_context(
                crate::util::arg_mut(arguments, 0)?.expect_mut_z_obj()?,
                &from_baggage_class,
                &from_key_class,
                &from_keys_class,
            )
        })
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string())),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            BAGGAGE_INTERFACE.to_string(),
        )));

    let empty_class = baggage_class.clone();
    class
        .add_static_method("getEmpty", Visibility::Public, move |_| {
            empty_baggage(&empty_class)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            BAGGAGE_INTERFACE.to_string(),
        )));

    let current_context_class = context_class.clone();
    let current_baggage_class = baggage_class.clone();
    let current_key_class = key_class.clone();
    let current_keys_class = keys_class.clone();
    class
        .add_static_method("getCurrent", Visibility::Public, move |_| {
            let mut context = current_context_value(&current_context_class)?;
            baggage_from_context(
                context.expect_mut_z_obj()?,
                &current_baggage_class,
                &current_key_class,
                &current_keys_class,
            )
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            BAGGAGE_INTERFACE.to_string(),
        )));

    class
        .add_static_method("getBuilder", Visibility::Public, move |_| {
            let object =
                phper::classes::ClassEntry::from_globals(BAGGAGE_BUILDER_CLASS)?.init_object()?;
            Ok::<_, phper::Error>(object)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            BAGGAGE_BUILDER_INTERFACE.to_string(),
        )));

    class
        .add_method("__construct", Visibility::Public, |this, arguments| {
            *this.as_mut_state() = arguments
                .first()
                .and_then(ZVal::as_z_arr)
                .map(entries_from_array)
                .unwrap_or_default();
            Ok::<_, phper::Error>(())
        })
        .argument(
            Argument::new("entries")
                .with_type_hint(ArgumentTypeHint::Array)
                .with_default_value("[]"),
        );

    let activate_context_class = context_class;
    class
        .add_method("activate", Visibility::Public, move |this, _| {
            let mut context = current_context_value(&activate_context_class)?;
            let mut with_baggage = context
                .expect_mut_z_obj()?
                .call("withContextValue", &mut [ZVal::from(this.to_ref_owned())])?;
            with_baggage.expect_mut_z_obj()?.call("activate", [])
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SCOPE_INTERFACE.to_string(),
        )));

    class
        .add_method("getEntry", Visibility::Public, |this, arguments| {
            let key = crate::util::arg(arguments, 0)?.expect_z_str()?.to_bytes();
            Ok::<_, phper::Error>(
                this.as_state()
                    .iter()
                    .find(|(candidate, _)| candidate.as_slice() == key)
                    .map(|(_, entry)| entry.clone()),
            )
        })
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(
            ReturnType::new(ReturnTypeHint::ClassEntry(ENTRY_CLASS.to_string())).allow_null(),
        );

    class
        .add_method("getValue", Visibility::Public, |this, arguments| {
            let key = crate::util::arg(arguments, 0)?.expect_z_str()?.to_bytes();
            let Some((_, entry)) = this
                .as_state()
                .iter()
                .find(|(candidate, _)| candidate.as_slice() == key)
            else {
                return Ok::<_, phper::Error>(ZVal::default());
            };
            let mut entry = entry.clone();
            entry.expect_mut_z_obj()?.call("getValue", [])
        })
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String));

    class
        .add_method("getAll", Visibility::Public, |this, _| {
            Ok::<_, std::convert::Infallible>(entries_to_array(this.as_state()))
        })
        .return_type(ReturnType::new(ReturnTypeHint::Iterable));

    class
        .add_method("isEmpty", Visibility::Public, |this, _| {
            Ok::<_, std::convert::Infallible>(this.as_state().is_empty())
        })
        .return_type(ReturnType::new(ReturnTypeHint::Bool));

    class
        .add_method("toBuilder", Visibility::Public, move |this, _| {
            let entries = ZVal::from(entries_to_array(this.as_state()));
            let object = phper::classes::ClassEntry::from_globals(BAGGAGE_BUILDER_CLASS)?
                .new_object([entries])?;
            Ok::<_, phper::Error>(object)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            BAGGAGE_BUILDER_INTERFACE.to_string(),
        )));

    class
        .add_method(
            "storeInContext",
            Visibility::Public,
            move |this, arguments| {
                let key = get_or_create_context_key(
                    &keys_class,
                    &key_class,
                    "baggage",
                    "opentelemetry-trace-baggage-key",
                )?;
                let baggage = ZVal::from(this.to_ref_owned());
                crate::util::arg_mut(arguments, 0)?
                    .expect_mut_z_obj()?
                    .call("with", &mut [key, baggage])
            },
        )
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string())),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_INTERFACE.to_string(),
        )));

    class
}
