use crate::context::{
    context::{ContextClass, current_context_value},
    propagation::text_map_propagator_interface::TEXT_MAP_INTERFACE,
};
use phper::{
    arrays::ZArray,
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};
use std::collections::HashSet;

const CLASS_NAME: &str = r"OpenTelemetry\Context\Propagation\MultiTextMapPropagator";

pub type MultiTextMapPropagatorClass = StateClass<MultiTextMapState>;

#[derive(Clone, Default)]
pub struct MultiTextMapState {
    propagators: Vec<ZVal>,
    fields: Vec<String>,
}

pub fn make_multi_text_map_propagator_class(
    interface: Interface,
    context_class: ContextClass,
) -> ClassEntity<MultiTextMapState> {
    let mut class: ClassEntity<MultiTextMapState> =
        ClassEntity::new_with_default_state_constructor(CLASS_NAME);
    class.set_final();
    class.state_cloner(Clone::clone);
    class.implements(interface);
    class
        .add_method("__construct", Visibility::Public, |this, arguments| {
            let propagators = crate::util::arg(arguments, 0)?.expect_z_arr()?;
            let mut seen = HashSet::new();
            for (_, propagator) in propagators.iter() {
                let mut propagator = propagator.clone();
                let fields = propagator.expect_mut_z_obj()?.call("fields", [])?;
                for (_, field) in fields.expect_z_arr()?.iter() {
                    if let Some(field) = field.as_z_str().and_then(|field| field.to_str().ok())
                        && seen.insert(field.to_string())
                    {
                        this.as_mut_state().fields.push(field.to_string());
                    }
                }
                this.as_mut_state().propagators.push(propagator);
            }
            Ok::<_, phper::Error>(())
        })
        .argument(Argument::new("propagators").with_type_hint(ArgumentTypeHint::Array));

    class
        .add_method("fields", Visibility::Public, |this, _| {
            let mut fields = ZArray::new();
            for field in &this.as_state().fields {
                fields.insert((), field.as_str());
            }
            Ok::<_, std::convert::Infallible>(fields)
        })
        .return_type(ReturnType::new(ReturnTypeHint::Array));

    class
        .add_method("inject", Visibility::Public, |this, arguments| {
            for propagator in &this.as_state().propagators {
                let mut propagator = propagator.clone();
                propagator.expect_mut_z_obj()?.call(
                    "inject",
                    &mut [
                        crate::util::arg(arguments, 0)?.clone(),
                        arguments.get(1).cloned().unwrap_or_default(),
                        arguments.get(2).cloned().unwrap_or_default(),
                    ],
                )?;
            }
            Ok::<_, phper::Error>(())
        })
        .argument(Argument::new("carrier").by_ref())
        .argument(
            Argument::new("setter")
                .with_type_hint(ArgumentTypeHint::ClassEntry(
                    r"OpenTelemetry\Context\Propagation\PropagationSetterInterface".to_string(),
                ))
                .allow_null()
                .with_default_value("NULL"),
        )
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(
                    r"OpenTelemetry\Context\ContextInterface".to_string(),
                ))
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    class
        .add_method("extract", Visibility::Public, move |this, arguments| {
            let mut context = match arguments.get(2).filter(|value| value.as_z_obj().is_some()) {
                Some(context) => context.clone(),
                None => current_context_value(&context_class)?,
            };
            for propagator in &this.as_state().propagators {
                let mut propagator = propagator.clone();
                context = propagator.expect_mut_z_obj()?.call(
                    "extract",
                    &mut [
                        crate::util::arg(arguments, 0)?.clone(),
                        arguments.get(1).cloned().unwrap_or_default(),
                        context,
                    ],
                )?;
            }
            Ok::<_, phper::Error>(context)
        })
        .argument(Argument::new("carrier"))
        .argument(
            Argument::new("getter")
                .with_type_hint(ArgumentTypeHint::ClassEntry(
                    r"OpenTelemetry\Context\Propagation\PropagationGetterInterface".to_string(),
                ))
                .allow_null()
                .with_default_value("NULL"),
        )
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(
                    r"OpenTelemetry\Context\ContextInterface".to_string(),
                ))
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\Context\ContextInterface".to_string(),
        )));

    class
}

#[allow(dead_code)]
fn _assert_interface_name() {
    let _ = TEXT_MAP_INTERFACE;
}
