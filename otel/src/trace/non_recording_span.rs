use crate::trace::{
    span::{SpanBaseClass},
    span_context::{SpanContextClass, init_span_context_object},
    span_context_interface::SPAN_CONTEXT_INTERFACE,
    span_interface::SPAN_INTERFACE,
};
use opentelemetry::trace::SpanContext;
use phper::{
    alloc::ToRefOwned,
    classes::{ClassEntity, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};
use std::convert::Infallible;

pub const NON_RECORDING_SPAN_CLASS_NAME: &str = r"OpenTelemetry\API\Trace\NonRecordingSpan";
pub type NonRecordingSpanClass = StateClass<Option<ZVal>>;

fn span_return() -> ReturnType {
    ReturnType::new(ReturnTypeHint::ClassEntry(SPAN_INTERFACE.to_string()))
}

pub fn make_non_recording_span_class(
    parent: SpanBaseClass,
    span_context_class: SpanContextClass,
) -> ClassEntity<Option<ZVal>> {
    let mut class = ClassEntity::new_with_default_state_constructor(NON_RECORDING_SPAN_CLASS_NAME);
    class.set_final();
    class.state_cloner(Clone::clone);
    class.extends(parent);

    class
        .add_method("__construct", Visibility::Public, |this, arguments| {
            *this.as_mut_state() = Some(crate::util::arg(arguments, 0)?.clone());
            Ok::<_, phper::Error>(())
        })
        .argument(
            Argument::new("context").with_type_hint(ArgumentTypeHint::ClassEntry(
                SPAN_CONTEXT_INTERFACE.to_string(),
            )),
        );

    class
        .add_method("getContext", Visibility::Public, move |this, _| {
            if let Some(context) = this.as_state() {
                return Ok::<_, phper::Error>(context.clone());
            }
            Ok(ZVal::from(init_span_context_object(
                &span_context_class,
                SpanContext::empty_context(),
                None,
            )?))
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_CONTEXT_INTERFACE.to_string(),
        )));
    class
        .add_method("isRecording", Visibility::Public, |_, _| {
            Ok::<_, Infallible>(false)
        })
        .return_type(ReturnType::new(ReturnTypeHint::Bool));
    class
        .add_method("setAttribute", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.to_ref_owned())
        })
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .argument(Argument::new("value"))
        .return_type(span_return());
    class
        .add_method("setAttributes", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.to_ref_owned())
        })
        .argument(Argument::new("attributes").with_type_hint(ArgumentTypeHint::Iterable))
        .return_type(span_return());
    class
        .add_method("addLink", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.to_ref_owned())
        })
        .argument(
            Argument::new("context").with_type_hint(ArgumentTypeHint::ClassEntry(
                SPAN_CONTEXT_INTERFACE.to_string(),
            )),
        )
        .argument(
            Argument::new("attributes")
                .with_type_hint(ArgumentTypeHint::Iterable)
                .with_default_value("[]"),
        )
        .return_type(span_return());
    class
        .add_method("addEvent", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.to_ref_owned())
        })
        .argument(Argument::new("name").with_type_hint(ArgumentTypeHint::String))
        .argument(
            Argument::new("attributes")
                .with_type_hint(ArgumentTypeHint::Iterable)
                .with_default_value("[]"),
        )
        .argument(
            Argument::new("timestamp")
                .with_type_hint(ArgumentTypeHint::Int)
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(span_return());
    class
        .add_method("recordException", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.to_ref_owned())
        })
        .argument(
            Argument::new("exception")
                .with_type_hint(ArgumentTypeHint::ClassEntry("Throwable".to_string())),
        )
        .argument(
            Argument::new("attributes")
                .with_type_hint(ArgumentTypeHint::Iterable)
                .with_default_value("[]"),
        )
        .return_type(span_return());
    class
        .add_method("updateName", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.to_ref_owned())
        })
        .argument(Argument::new("name").with_type_hint(ArgumentTypeHint::String))
        .return_type(span_return());
    class
        .add_method("setStatus", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.to_ref_owned())
        })
        .argument(Argument::new("code").with_type_hint(ArgumentTypeHint::String))
        .argument(
            Argument::new("description")
                .with_type_hint(ArgumentTypeHint::String)
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(span_return());
    class
        .add_method("end", Visibility::Public, |_, _| Ok::<_, Infallible>(() ))
        .argument(
            Argument::new("endEpochNanos")
                .with_type_hint(ArgumentTypeHint::Int)
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    class
}
