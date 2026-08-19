use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};
use std::{
    convert::Infallible,
};
use opentelemetry::{
    trace::{
        SpanBuilder,
        Tracer,
    }
};
use opentelemetry_sdk::trace::SdkTracer;
use crate::trace::span_builder::{
    SpanBuilderState,
    SpanBuilderClass,
};

pub type TracerClass = StateClass<Option<SdkTracer>>;

const TRACER_CLASS_NAME: &str = r"OpenTelemetry\API\Trace\Tracer";

pub fn make_tracer_class(
    span_builder_class: SpanBuilderClass,
    tracer_interface: Interface,
) -> ClassEntity<Option<SdkTracer>> {
    let mut class =
        ClassEntity::<Option<SdkTracer>>::new_with_default_state_constructor(TRACER_CLASS_NAME);

    class.implements(tracer_interface);
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method("spanBuilder", Visibility::Public, move |this, arguments| {
            let name = crate::util::arg(arguments, 0)?.expect_z_str()?.to_str()?;
            let mut object = span_builder_class.init_object()?;
            // A tracer from the no-op provider keeps the builder stateless so
            // the disabled path never copies names or touches the SDK.
            if let Some(tracer) = this.as_state().as_ref() {
                let span_builder: SpanBuilder = tracer.span_builder(name.to_string());
                *object.as_mut_state() = SpanBuilderState::new(span_builder, tracer.clone());
            }
            Ok::<_, phper::Error>(object)
        })
        .argument(Argument::new("spanName").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(String::from(
            r"OpenTelemetry\API\Trace\SpanBuilderInterface",
        ))));

    class
        .add_method("isEnabled", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.as_state().is_some())
        })
        .return_type(ReturnType::new(ReturnTypeHint::Bool));

    class
}
