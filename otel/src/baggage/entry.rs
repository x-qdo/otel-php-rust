use crate::baggage::interfaces::METADATA_INTERFACE;
use phper::{
    classes::{ClassEntity, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};

pub const ENTRY_CLASS: &str = r"OpenTelemetry\API\Baggage\Entry";

#[derive(Clone, Default)]
pub struct EntryState {
    value: ZVal,
    metadata: ZVal,
}

pub type EntryClass = StateClass<EntryState>;

pub fn init_entry_object(
    class: &EntryClass,
    value: ZVal,
    metadata: ZVal,
) -> phper::Result<phper::objects::StateObject<EntryState>> {
    let mut object = class.init_object()?;
    *object.as_mut_state() = EntryState { value, metadata };
    Ok(object)
}

pub fn make_entry_class() -> ClassEntity<EntryState> {
    let mut class = ClassEntity::new_with_default_state_constructor(ENTRY_CLASS);
    class.set_final();
    class.state_cloner(Clone::clone);

    class
        .add_method("__construct", Visibility::Public, |this, arguments| {
            *this.as_mut_state() = EntryState {
                value: crate::util::arg(arguments, 0)?.clone(),
                metadata: crate::util::arg(arguments, 1)?.clone(),
            };
            Ok::<_, phper::Error>(())
        })
        .argument(Argument::new("value").with_type_hint(ArgumentTypeHint::Mixed))
        .argument(
            Argument::new("metadata")
                .with_type_hint(ArgumentTypeHint::ClassEntry(METADATA_INTERFACE.to_string())),
        );

    class
        .add_method("getValue", Visibility::Public, |this, _| {
            Ok::<_, std::convert::Infallible>(this.as_state().value.clone())
        })
        .return_type(ReturnType::new(ReturnTypeHint::Mixed));
    class
        .add_method("getMetadata", Visibility::Public, |this, _| {
            Ok::<_, std::convert::Infallible>(this.as_state().metadata.clone())
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            METADATA_INTERFACE.to_string(),
        )));

    class
}
