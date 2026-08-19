use phper::{
    classes::{ClassEntity, StateClass, Visibility},
};
use std::{
    convert::Infallible,
};
use opentelemetry::trace::{
    SpanContext,
    TraceFlags,
    TraceState,
    SpanId,
    TraceId,
};

/// Native state is `None` only for an object created without its state, e.g.
/// via `ReflectionClass::newInstanceWithoutConstructor()`; such an object
/// behaves as the invalid span context.
fn with_state<T>(state: &Option<SpanContext>, invalid: T, f: impl FnOnce(&SpanContext) -> T) -> T {
    state.as_ref().map_or(invalid, f)
}

const SPAN_CONTEXT_CLASS_NAME: &str = r"OpenTelemetry\API\Trace\SpanContext";

pub type SpanContextClass = StateClass<Option<SpanContext>>;

pub fn make_span_context_class() -> ClassEntity<Option<SpanContext>> {
    let mut class =
        ClassEntity::<Option<SpanContext>>::new_with_default_state_constructor(SPAN_CONTEXT_CLASS_NAME);
    class.set_final();
    let span_context_class = class.bound_class();

    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    let span_context_class_invalid = span_context_class.clone();
    class.add_static_method("getInvalid", Visibility::Public, move |_arguments| {
        let span_context = SpanContext::empty_context();
        let mut object = span_context_class_invalid.clone().init_object()?;
        *object.as_mut_state() = Some(span_context);
        Ok::<_, phper::Error>(object)
    });

    let span_context_class_create = span_context_class.clone();
    class.add_static_method("create", Visibility::Public, move |arguments| {
        let trace_id = crate::util::arg(arguments, 0)?.expect_z_str()?.to_str()?;
        let span_id = crate::util::arg(arguments, 1)?.expect_z_str()?.to_str()?;
        let span_context = SpanContext::new(
            TraceId::from_hex(trace_id).map_err(|_| phper::Error::boxed("Invalid trace id format"))?,
            SpanId::from_hex(span_id).map_err(|_| phper::Error::boxed("Invalid trace id format"))?,
            TraceFlags::SAMPLED, //todo
            false,
            TraceState::default(), //todo
        );
        let mut object = span_context_class_create.init_object()?;
        *object.as_mut_state() = Some(span_context);
        Ok::<_, phper::Error>(object)
    });

    let span_context_class_create_from_remote_parent = span_context_class.clone();
    class.add_static_method("createFromRemoteParent", Visibility::Public, move |arguments| {
        let trace_id = crate::util::arg(arguments, 0)?.expect_z_str()?.to_str()?;
        let span_id = crate::util::arg(arguments, 1)?.expect_z_str()?.to_str()?;
        let span_context = SpanContext::new(
            TraceId::from_hex(trace_id).map_err(|_| phper::Error::boxed("Invalid trace id format"))?,
            SpanId::from_hex(span_id).map_err(|_| phper::Error::boxed("Invalid trace id format"))?,
            TraceFlags::SAMPLED, //todo
            true,
            TraceState::default(), //todo
        );
        let mut object = span_context_class_create_from_remote_parent.clone().init_object()?;
        *object.as_mut_state() = Some(span_context);
        Ok::<_, phper::Error>(object)
    });

    class.add_method("isValid", Visibility::Public, |this, _| {
        let state = this.as_state();
        let is_valid = with_state(state, false, SpanContext::is_valid);
        Ok::<_, phper::Error>(is_valid)
    });

    class.add_method("getTraceId", Visibility::Public, |this, _| {
        let state = this.as_state();
        let trace_id = with_state(state, TraceId::INVALID, SpanContext::trace_id).to_string();
        Ok::<_, phper::Error>(trace_id)
    });

    class.add_method("getSpanId", Visibility::Public, |this, _| {
        let state = this.as_state();
        let span_id = with_state(state, SpanId::INVALID, SpanContext::span_id).to_string();
        Ok::<_, phper::Error>(span_id)
    });

    class.add_method("isRemote", Visibility::Public, |this, _| {
        let state = this.as_state();
        let is_remote = with_state(state, false, SpanContext::is_remote);
        Ok::<_, phper::Error>(is_remote)
    });

    class.add_method("isSampled", Visibility::Public, |this, _| {
        let state = this.as_state();
        let is_sampled = with_state(state, false, SpanContext::is_sampled);
        Ok::<_, phper::Error>(is_sampled)
    });

    class
}
