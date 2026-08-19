use crate::baggage::{
    baggage::{BAGGAGE_CLASS, BaggageEntries, entries_from_array, entries_to_array},
    entry::{EntryClass, init_entry_object},
    interfaces::{BAGGAGE_BUILDER_INTERFACE, BAGGAGE_INTERFACE, METADATA_INTERFACE},
    metadata::{MetadataClass, empty_metadata},
};
use phper::{
    alloc::ToRefOwned,
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};

pub const BAGGAGE_BUILDER_CLASS: &str = r"OpenTelemetry\API\Baggage\BaggageBuilder";

pub type BaggageBuilderClass = StateClass<BaggageEntries>;

pub fn init_baggage_builder_object(
    class: &BaggageBuilderClass,
    entries: BaggageEntries,
) -> phper::Result<phper::objects::StateObject<BaggageEntries>> {
    let mut object = class.init_object()?;
    *object.as_mut_state() = entries;
    Ok(object)
}

pub fn make_baggage_builder_class(
    interface: Interface,
    entry_class: EntryClass,
    metadata_class: MetadataClass,
) -> ClassEntity<BaggageEntries> {
    let mut class = ClassEntity::new_with_default_state_constructor(BAGGAGE_BUILDER_CLASS);
    class.set_final();
    class.state_cloner(Clone::clone);
    class.implements(interface);

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

    class
        .add_method("set", Visibility::Public, move |this, arguments| {
            let key = crate::util::arg(arguments, 0)?
                .expect_z_str()?
                .to_bytes()
                .to_vec();
            if key.is_empty() {
                return Ok::<_, phper::Error>(ZVal::from(this.to_ref_owned()));
            }
            let metadata = match arguments.get(2) {
                Some(value) if value.as_z_obj().is_some() => value.clone(),
                _ => empty_metadata(&metadata_class)?,
            };
            let entry = ZVal::from(init_entry_object(
                &entry_class,
                crate::util::arg(arguments, 1)?.clone(),
                metadata,
            )?);
            if let Some((_, existing)) = this
                .as_mut_state()
                .iter_mut()
                .find(|(candidate, _)| candidate == &key)
            {
                *existing = entry;
            } else {
                this.as_mut_state().push((key, entry));
            }
            Ok(ZVal::from(this.to_ref_owned()))
        })
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .argument(Argument::new("value"))
        .argument(
            Argument::new("metadata")
                .with_type_hint(ArgumentTypeHint::ClassEntry(METADATA_INTERFACE.to_string()))
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            BAGGAGE_BUILDER_INTERFACE.to_string(),
        )));

    class
        .add_method("remove", Visibility::Public, |this, arguments| {
            let key = crate::util::arg(arguments, 0)?.expect_z_str()?.to_bytes();
            this.as_mut_state()
                .retain(|(candidate, _)| candidate.as_slice() != key);
            Ok::<_, phper::Error>(ZVal::from(this.to_ref_owned()))
        })
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            BAGGAGE_BUILDER_INTERFACE.to_string(),
        )));

    class
        .add_method("build", Visibility::Public, |this, _| {
            let entries = ZVal::from(entries_to_array(this.as_state()));
            let object =
                phper::classes::ClassEntry::from_globals(BAGGAGE_CLASS)?.new_object([entries])?;
            Ok::<_, phper::Error>(object)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            BAGGAGE_INTERFACE.to_string(),
        )));

    class
}
