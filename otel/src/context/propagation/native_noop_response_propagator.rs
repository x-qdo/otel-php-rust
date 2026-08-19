use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};

const CLASS_NAME: &str = r"OpenTelemetry\Context\Propagation\NativeNoopResponsePropagator";

pub type NativeNoopResponsePropagatorClass = StateClass<()>;

pub fn make_native_noop_response_propagator_class(interface: Interface) -> ClassEntity<()> {
    let mut class = ClassEntity::new_with_default_state_constructor(CLASS_NAME);
    class.set_final();
    class.state_cloner(Clone::clone);
    class.implements(interface);
    class
        .add_method("inject", Visibility::Public, |_, _| {
            Ok::<_, std::convert::Infallible>(())
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
}
