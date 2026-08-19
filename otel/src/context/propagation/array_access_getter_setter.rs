use phper::{
    arrays::{IterKey, ZArray},
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::{ZVal, ZValRef},
};

const CLASS_NAME: &str = r"OpenTelemetry\Context\Propagation\ArrayAccessGetterSetter";
pub type ArrayAccessGetterSetterClass = StateClass<()>;

pub fn array_access_getter_setter_instance(
    class: &ArrayAccessGetterSetterClass,
) -> phper::Result<ZVal> {
    if let Some(value) = class
        .as_class_entry()
        .get_static_property("instance")
        .filter(|value| value.as_z_obj().is_some())
    {
        return Ok(value.clone());
    }
    let value = ZVal::from(class.init_object()?);
    class
        .as_class_entry()
        .set_static_property("instance", value.clone());
    Ok(value)
}

pub fn make_array_access_getter_setter_class(
    extended_getter: Interface,
    setter: Interface,
) -> ClassEntity<()> {
    let mut class = ClassEntity::new_with_default_state_constructor(CLASS_NAME);
    class.set_final();
    class.implements(extended_getter);
    class.implements(setter);
    class.add_static_property("instance", Visibility::Private, ());
    let class_ref = class.bound_class();

    class
        .add_static_method("getInstance", Visibility::Public, move |_| {
            array_access_getter_setter_instance(&class_ref)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            "self".to_string(),
        )));

    class
        .add_method("keys", Visibility::Public, |_, arguments| {
            let carrier = carrier_array(crate::util::arg(arguments, 0)?)?;
            let mut result = ZArray::new();
            for (key, _) in carrier.expect_z_arr()?.iter() {
                let key = match key {
                    IterKey::Index(key) => key.to_string(),
                    IterKey::ZStr(key) => key.to_str()?.to_string(),
                };
                result.insert((), key);
            }
            Ok::<_, phper::Error>(result)
        })
        .argument(Argument::new("carrier"))
        .return_type(ReturnType::new(ReturnTypeHint::Array));

    class
        .add_method("get", Visibility::Public, |_, arguments| {
            let carrier = carrier_array(crate::util::arg(arguments, 0)?)?;
            let key = crate::util::arg(arguments, 1)?.expect_z_str()?.to_str()?;
            Ok::<_, phper::Error>(first_string(find_value(carrier.expect_z_arr()?, key)))
        })
        .argument(Argument::new("carrier"))
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::String).allow_null());

    class
        .add_method("getAll", Visibility::Public, |_, arguments| {
            let carrier = carrier_array(crate::util::arg(arguments, 0)?)?;
            let key = crate::util::arg(arguments, 1)?.expect_z_str()?.to_str()?;
            let mut result = ZArray::new();
            if let Some(value) = find_value(carrier.expect_z_arr()?, key) {
                match value.to_value()? {
                    ZValRef::Str(value) => result.insert((), value.to_str()?.to_string()),
                    ZValRef::Arr(values) => {
                        for (_, value) in values.iter() {
                            if let Some(value) = value.as_z_str() {
                                result.insert((), value.to_str()?.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok::<_, phper::Error>(result)
        })
        .argument(Argument::new("carrier"))
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::Array));

    class
        .add_method("set", Visibility::Public, |_, arguments| {
            let key = crate::util::arg(arguments, 1)?
                .expect_z_str()?
                .to_str()?
                .to_string();
            if key.is_empty() {
                return Err(phper::Error::boxed("Unable to set value with an empty key"));
            }
            let value = crate::util::arg(arguments, 2)?
                .expect_z_str()?
                .to_str()?
                .to_string();
            let carrier = crate::util::arg_mut(arguments, 0)?.expect_mut_z_ref()?;
            let target = carrier.val_mut();
            if target.as_z_arr().is_some() {
                let mut copy = target.clone();
                let array = copy.expect_mut_z_arr()?;
                if let Some(existing) = resolve_key(array, &key)
                    && existing != key
                {
                    array.remove(existing.as_str());
                }
                array.insert(key.as_str(), value);
                *target = copy;
                return Ok::<_, phper::Error>(());
            }
            let resolved = crate::util::zval_iterable_to_array(target)
                .map_err(|_| phper::Error::boxed("Unsupported carrier type"))?;
            let resolved = resolve_key(resolved.expect_z_arr()?, &key);
            if let Some(object) = target.as_mut_z_obj() {
                if let Some(existing) = resolved
                    && existing != key
                {
                    object.call("offsetUnset", &mut [ZVal::from(existing)])?;
                }
                object.call("offsetSet", &mut [ZVal::from(key), ZVal::from(value)])?;
                return Ok(());
            }
            Err(phper::Error::boxed("Unsupported carrier type"))
        })
        .argument(Argument::new("carrier").by_ref())
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .argument(Argument::new("value").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    class
}

fn carrier_array(carrier: &ZVal) -> phper::Result<ZVal> {
    crate::util::zval_iterable_to_array(carrier)
        .map_err(|_| phper::Error::boxed("Unsupported carrier type"))
}

fn resolve_key(array: &phper::arrays::ZArr, key: &str) -> Option<String> {
    array.iter().find_map(|(candidate, _)| {
        let candidate = match candidate {
            IterKey::Index(candidate) => candidate.to_string(),
            IterKey::ZStr(candidate) => candidate.to_str().ok()?.to_string(),
        };
        candidate.eq_ignore_ascii_case(key).then_some(candidate)
    })
}

fn find_value<'a>(array: &'a phper::arrays::ZArr, key: &str) -> Option<&'a ZVal> {
    array.iter().find_map(|(candidate, value)| {
        let matches = match candidate {
            IterKey::Index(candidate) => candidate.to_string().eq_ignore_ascii_case(key),
            IterKey::ZStr(candidate) => candidate
                .to_str()
                .is_ok_and(|candidate| candidate.eq_ignore_ascii_case(key)),
        };
        matches.then_some(value)
    })
}

fn first_string(value: Option<&ZVal>) -> Option<String> {
    match value?.to_value().ok()? {
        ZValRef::Str(value) => value.to_str().ok().map(str::to_string),
        ZValRef::Arr(values) => values
            .iter()
            .find_map(|(_, value)| value.as_z_str()?.to_str().ok().map(str::to_string)),
        _ => None,
    }
}
