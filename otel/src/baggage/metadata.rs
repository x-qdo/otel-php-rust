use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};

pub const METADATA_CLASS: &str = r"OpenTelemetry\API\Baggage\Metadata";

pub type MetadataClass = StateClass<Vec<u8>>;

pub fn init_metadata_object(
    class: &MetadataClass,
    metadata: Vec<u8>,
) -> phper::Result<phper::objects::StateObject<Vec<u8>>> {
    let mut object = class.init_object()?;
    *object.as_mut_state() = metadata;
    Ok(object)
}

pub fn empty_metadata(class: &MetadataClass) -> phper::Result<ZVal> {
    if let Some(value) = class
        .as_class_entry()
        .get_static_property("instance")
        .filter(|value| value.as_z_obj().is_some())
    {
        return Ok(value.clone());
    }
    let value = ZVal::from(init_metadata_object(class, Vec::new())?);
    class
        .as_class_entry()
        .set_static_property("instance", value.clone());
    Ok(value)
}

pub fn make_metadata_class(interface: Interface) -> ClassEntity<Vec<u8>> {
    let mut class = ClassEntity::new_with_default_state_constructor(METADATA_CLASS);
    class.set_final();
    class.state_cloner(Clone::clone);
    class.implements(interface);
    class.add_static_property("instance", Visibility::Private, ());
    let metadata_class = class.bound_class();

    let empty_class = metadata_class.clone();
    class
        .add_static_method("getEmpty", Visibility::Public, move |_| {
            empty_metadata(&empty_class)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            METADATA_CLASS.to_string(),
        )));

    class
        .add_method("__construct", Visibility::Public, |this, arguments| {
            *this.as_mut_state() = crate::util::arg(arguments, 0)?
                .expect_z_str()?
                .to_bytes()
                .to_vec();
            Ok::<_, phper::Error>(())
        })
        .argument(Argument::new("metadata").with_type_hint(ArgumentTypeHint::String));

    class
        .add_method("getValue", Visibility::Public, |this, _| {
            Ok::<_, std::convert::Infallible>(ZVal::from(this.as_state().clone()))
        })
        .return_type(ReturnType::new(ReturnTypeHint::String));

    class
}
