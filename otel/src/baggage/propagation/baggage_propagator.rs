use crate::{
    baggage::{
        baggage::{BaggageClass, baggage_from_context},
        baggage_builder::{BaggageBuilderClass, init_baggage_builder_object},
        metadata::MetadataClass,
        propagation::parser::parse_into_builder,
    },
    context::{
        context_class::{ContextClass, current_context_value},
        context_key::{ContextKeyClass, ContextKeysClass},
        propagation::array_access_getter_setter::{
            ArrayAccessGetterSetterClass, array_access_getter_setter_instance,
        },
    },
};
use phper::{
    arrays::{IterKey, ZArray},
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};

pub const BAGGAGE_PROPAGATOR_CLASS: &str =
    r"OpenTelemetry\API\Baggage\Propagation\BaggagePropagator";
const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";
const GETTER_INTERFACE: &str = r"OpenTelemetry\Context\Propagation\PropagationGetterInterface";
const SETTER_INTERFACE: &str = r"OpenTelemetry\Context\Propagation\PropagationSetterInterface";

pub type BaggagePropagatorClass = StateClass<()>;

fn url_encode(value: &[u8]) -> Vec<u8> {
    fn hex_digit(value: u8) -> u8 {
        match value {
            0..=9 => b'0' + value,
            _ => b'A' + value.saturating_sub(10),
        }
    }

    let mut encoded = Vec::with_capacity(value.len());
    for byte in value {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                encoded.push(*byte);
            }
            b' ' => encoded.push(b'+'),
            byte => {
                encoded.push(b'%');
                encoded.push(hex_digit(byte >> 4));
                encoded.push(hex_digit(byte & 0x0f));
            }
        }
    }
    encoded
}

fn selected_context(
    arguments: &[ZVal],
    index: usize,
    context_class: &ContextClass,
) -> phper::Result<ZVal> {
    if let Some(context) = arguments
        .get(index)
        .filter(|value| value.as_z_obj().is_some())
    {
        return Ok(context.clone());
    }
    current_context_value(context_class)
}

fn selected_accessor(
    arguments: &[ZVal],
    index: usize,
    default_class: &ArrayAccessGetterSetterClass,
) -> phper::Result<ZVal> {
    if let Some(accessor) = arguments
        .get(index)
        .filter(|value| value.as_z_obj().is_some())
    {
        return Ok(accessor.clone());
    }
    array_access_getter_setter_instance(default_class)
}

fn baggage_header(baggage: &mut ZVal) -> phper::Result<Vec<u8>> {
    if baggage
        .expect_mut_z_obj()?
        .call("isEmpty", [])?
        .as_bool()
        .unwrap_or(false)
    {
        return Ok(Vec::new());
    }

    let entries = baggage.expect_mut_z_obj()?.call("getAll", [])?;
    let entries = crate::util::zval_iterable_to_array(&entries)?;
    let mut header = Vec::new();
    for (key, value) in entries.expect_z_arr()?.iter() {
        if !header.is_empty() {
            header.push(b',');
        }
        match key {
            IterKey::Index(index) => header.extend_from_slice(index.to_string().as_bytes()),
            IterKey::ZStr(key) => header.extend_from_slice(key.to_bytes()),
        }
        header.push(b'=');

        let mut entry = value.clone();
        let entry = entry.expect_mut_z_obj()?;
        let value = entry.call("getValue", [])?;
        let value = phper::functions::call("strval", &mut [value])?;
        header.extend_from_slice(&url_encode(value.expect_z_str()?.to_bytes()));

        let mut metadata = entry.call("getMetadata", [])?;
        let metadata = metadata.expect_mut_z_obj()?.call("getValue", [])?;
        let metadata = metadata.expect_z_str()?.to_bytes();
        if !metadata.is_empty() && metadata != b"0" {
            header.push(b';');
            header.extend_from_slice(metadata);
        }
    }
    Ok(header)
}

#[allow(clippy::too_many_arguments)]
pub fn make_baggage_propagator_class(
    text_map_propagator_interface: Interface,
    context_class: ContextClass,
    key_class: ContextKeyClass,
    keys_class: ContextKeysClass,
    baggage_class: BaggageClass,
    builder_class: BaggageBuilderClass,
    metadata_class: MetadataClass,
    array_access_class: ArrayAccessGetterSetterClass,
) -> ClassEntity<()> {
    let mut class = ClassEntity::new_with_default_state_constructor(BAGGAGE_PROPAGATOR_CLASS);
    class.set_final();
    class.state_cloner(Clone::clone);
    class.implements(text_map_propagator_interface);
    class.add_constant("BAGGAGE", "baggage");
    class.add_static_property("instance", Visibility::Private, ());
    let propagator_class = class.bound_class();

    let instance_class = propagator_class.clone();
    let instance_owner = propagator_class.clone();
    class
        .add_static_method("getInstance", Visibility::Public, move |_| {
            if let Some(value) = instance_owner
                .as_class_entry()
                .get_static_property("instance")
                .filter(|value| value.as_z_obj().is_some())
            {
                return Ok::<_, phper::Error>(value.clone());
            }
            let value = ZVal::from(instance_class.init_object()?);
            instance_owner
                .as_class_entry()
                .set_static_property("instance", value.clone());
            Ok(value)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            "self".to_string(),
        )));

    class
        .add_method("fields", Visibility::Public, |_, _| {
            let mut fields = ZArray::new();
            fields.insert((), "baggage");
            Ok::<_, std::convert::Infallible>(fields)
        })
        .return_type(ReturnType::new(ReturnTypeHint::Array));

    let inject_context_class = context_class.clone();
    let inject_key_class = key_class.clone();
    let inject_keys_class = keys_class.clone();
    let inject_baggage_class = baggage_class.clone();
    let inject_array_access_class = array_access_class.clone();
    class
        .add_method("inject", Visibility::Public, move |_, arguments| {
            let mut context = selected_context(arguments, 2, &inject_context_class)?;
            let mut baggage = baggage_from_context(
                context.expect_mut_z_obj()?,
                &inject_baggage_class,
                &inject_key_class,
                &inject_keys_class,
            )?;
            let header = baggage_header(&mut baggage)?;
            if header.is_empty() || header == b"0" {
                return Ok::<_, phper::Error>(());
            }
            let mut setter = selected_accessor(arguments, 1, &inject_array_access_class)?;
            setter.expect_mut_z_obj()?.call(
                "set",
                &mut [
                    crate::util::arg(arguments, 0)?.clone(),
                    ZVal::from("baggage"),
                    ZVal::from(header),
                ],
            )?;
            Ok(())
        })
        .argument(Argument::new("carrier").by_ref())
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

    let extract_context_class = context_class;
    class
        .add_method("extract", Visibility::Public, move |_, arguments| {
            let context = selected_context(arguments, 2, &extract_context_class)?;
            let mut getter = selected_accessor(arguments, 1, &array_access_class)?;
            let header = getter.expect_mut_z_obj()?.call(
                "get",
                &mut [
                    crate::util::arg(arguments, 0)?.clone(),
                    ZVal::from("baggage"),
                ],
            )?;
            let Some(header) = header.as_z_str() else {
                return Ok::<_, phper::Error>(context);
            };
            if header.is_empty() || header.to_bytes() == b"0" {
                return Ok(context);
            }
            let header = header.to_bytes().to_vec();
            let mut builder = init_baggage_builder_object(&builder_class, Vec::new())?;
            parse_into_builder(&header, &mut builder, &metadata_class)?;
            let baggage = builder.call("build", [])?;
            let mut context = context;
            context
                .expect_mut_z_obj()?
                .call("withContextValue", &mut [baggage])
        })
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

    class
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encode_matches_php_urlencode() {
        assert_eq!(url_encode(b"hello world!~"), b"hello+world%21%7E");
    }
}
