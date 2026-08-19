use crate::{
    context::{
        context::{ContextClass, get_instance_id},
        scope::ScopeClass,
        storage,
    },
    trace::{
        span::{SpanClass, current_span_object, span_object_from_context},
        span_context::SpanContextClass,
    },
};
use opentelemetry::{
    Context,
    trace::{SpanContext, TraceContextExt, noop::NoopSpan},
};
use opentelemetry_sdk::trace::Span as SdkSpan;
use phper::{
    alloc::ToRefOwned,
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    objects::ZObj,
    types::ReturnTypeHint,
};
use std::{convert::Infallible, sync::Arc};

const NON_RECORDING_SPAN_CLASS_NAME: &str = r"OpenTelemetry\API\Trace\NonRecordingSpan";

pub type NonRecordingSpanClass = StateClass<Option<SdkSpan>>; //never Some, but must match other implementations

pub fn make_non_recording_span_class(
    scope_class: ScopeClass,
    span_context_class: SpanContextClass,
    context_class: ContextClass,
    span_class: SpanClass,
    span_interface: &Interface,
) -> ClassEntity<Option<SdkSpan>> {
    let mut class = ClassEntity::<Option<SdkSpan>>::new_with_default_state_constructor(
        NON_RECORDING_SPAN_CLASS_NAME,
    );
    let _span_class = class.bound_class();

    class.implements(span_interface.clone());

    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method("isRecording", Visibility::Public, |_, _| {
            Ok::<_, phper::Error>(false)
        })
        .return_type(ReturnType::new(ReturnTypeHint::Bool));

    class.add_method("end", Visibility::Public, |_, _| -> phper::Result<()> {
        Ok(())
    });

    class
        .add_method("setStatus", Visibility::Public, |this, _| {
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("code"))
        .argument(Argument::new("description").optional());

    class.add_method("setAttribute", Visibility::Public, |this, _| {
        Ok::<_, phper::Error>(this.to_ref_owned())
    });

    class.add_method("setAttributes", Visibility::Public, |this, _| {
        Ok::<_, phper::Error>(this.to_ref_owned())
    });

    class.add_method("updateName", Visibility::Public, |this, _| {
        Ok::<_, phper::Error>(this.to_ref_owned())
    });

    class.add_method("recordException", Visibility::Public, |this, _| {
        Ok::<_, phper::Error>(this.to_ref_owned())
    });

    class.add_method("addLink", Visibility::Public, |this, _| {
        Ok::<_, phper::Error>(this.to_ref_owned())
    });

    class.add_method("addEvent", Visibility::Public, |this, _| {
        Ok::<_, phper::Error>(this.to_ref_owned())
    });

    class.add_method("getContext", Visibility::Public, move |_this, _| {
        let mut object = span_context_class.init_object()?;
        *object.as_mut_state() = Some(SpanContext::empty_context());

        Ok::<_, phper::Error>(object)
    });

    class.add_static_method("getCurrent", Visibility::Public, {
        let span_class = span_class.clone();
        move |_| current_span_object(&span_class)
    });

    // Activating a non-recording span makes the current span invalid, exactly
    // like the official API's NonRecordingSpan::activate().
    class.add_method("activate", Visibility::Public, move |_, _| {
        let context = Arc::new(Context::current().with_span(NoopSpan::DEFAULT));
        let instance_id = storage::store_context_instance(context);
        storage::attach_context(instance_id).map_err(phper::Error::boxed)?;

        let mut object = scope_class.init_object()?;
        object.as_mut_state().context = storage::get_context_instance(instance_id);
        object.set_property("context_id", instance_id.unwrap_or(0) as i64);
        Ok::<_, phper::Error>(object)
    });

    class.add_method("storeInContext", Visibility::Public, move |_, arguments| {
        let context_obj: &mut ZObj = arguments[0].expect_mut_z_obj()?;
        let parent = storage::resolve_context(get_instance_id(context_obj));
        let context = Arc::new(parent.with_span(NoopSpan::DEFAULT));
        let instance_id = storage::store_context_instance(context.clone());

        let mut object = context_class.init_object()?;
        *object.as_mut_state() = Some(context);
        object.set_property("context_id", instance_id.unwrap_or(0) as i64);
        Ok::<_, phper::Error>(object)
    });

    class.add_static_method("fromContext", Visibility::Public, move |arguments| {
        span_object_from_context(&span_class, arguments[0].expect_mut_z_obj()?)
    });

    class
}
