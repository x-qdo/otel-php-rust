use crate::{
    context::{
        context_class::{ContextClass, get_instance_id, init_context_object, native_context_from_object},
        context_key::{ContextKeyClass, ContextKeysClass, get_or_create_context_key},
        storage,
    },
    trace::{
        local_root_span::local_root_key,
        span_context::{SpanContextClass, init_span_context_object, span_context_from_object},
        span_interface::SPAN_INTERFACE,
        trace_state::TraceStateClass,
    },
    util,
};
use opentelemetry::trace::{
    Span as OtelSpan, SpanContext, Status, TraceContextExt, noop::NoopSpan,
};
use opentelemetry::Context as OtelContext;
use opentelemetry_sdk::trace::Span as SdkSpan;
use phper::{
    alloc::ToRefOwned,
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    objects::ZObj,
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};
use std::{borrow::Cow, convert::Infallible, rc::Rc, time::{Duration, UNIX_EPOCH}};

pub const SPAN_CLASS_NAME: &str = r"OpenTelemetry\API\Trace\Span";
pub const NATIVE_SPAN_CLASS_NAME: &str = r"OpenTelemetry\API\Trace\NativeSpan";
const NON_RECORDING_SPAN_CLASS_NAME: &str = r"OpenTelemetry\API\Trace\NonRecordingSpan";
const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";
const SCOPE_INTERFACE: &str = r"OpenTelemetry\Context\ScopeInterface";
const SPAN_CONTEXT_INTERFACE: &str = r"OpenTelemetry\API\Trace\SpanContextInterface";

pub type SpanBaseClass = StateClass<()>;
pub type SpanClass = StateClass<Option<SdkSpan>>;

fn span_return() -> ReturnType {
    ReturnType::new(ReturnTypeHint::ClassEntry(SPAN_INTERFACE.to_string()))
}

fn iterable_attributes(
    value: &ZVal,
    destination: util::AttributeDestination,
) -> phper::Result<Vec<opentelemetry::KeyValue>> {
    let array = util::zval_iterable_to_array(value)?;
    Ok(util::zval_arr_to_key_value_vec(array.expect_z_arr()?, destination))
}

fn optional_iterable_attributes(
    value: Option<&ZVal>,
    destination: util::AttributeDestination,
) -> phper::Result<Vec<opentelemetry::KeyValue>> {
    value
        .map(|value| iterable_attributes(value, destination))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn timestamp(value: Option<&ZVal>) -> Option<std::time::SystemTime> {
    value
        .and_then(ZVal::as_long)
        .and_then(|nanos| (nanos >= 0).then_some(UNIX_EPOCH + Duration::from_nanos(nanos as u64)))
}

pub(crate) fn new_non_recording_span(context: ZVal) -> phper::Result<ZVal> {
    let object = phper::classes::ClassEntry::from_globals(NON_RECORDING_SPAN_CLASS_NAME)?
        .new_object([context])?;
    Ok(ZVal::from(object))
}

pub(crate) fn otel_context_from_php(
    context: &mut ZObj,
    key_class: &ContextKeyClass,
    keys_class: &ContextKeysClass,
) -> phper::Result<OtelContext> {
    if let Some(native) = native_context_from_object(context) {
        return Ok((**native).clone());
    }

    let span_key = get_or_create_context_key(
        keys_class,
        key_class,
        "span",
        "opentelemetry-trace-span-key",
    )?;
    let mut span = context.call("get", &mut [span_key])?;
    let Some(span) = span.as_mut_z_obj() else {
        return Ok(OtelContext::new());
    };
    let mut span_context = span.call("getContext", [])?;
    let span_context = span_context_from_object(span_context.expect_mut_z_obj()?)?;
    if span_context.is_valid() {
        Ok(OtelContext::new().with_remote_span_context(span_context))
    } else {
        Ok(OtelContext::new())
    }
}

fn invalid_span(
    span_base: &SpanBaseClass,
    span_context_class: &SpanContextClass,
) -> phper::Result<ZVal> {
    if let Some(value) = span_base
        .as_class_entry()
        .get_static_property("invalidSpan")
        .filter(|value| value.as_z_obj().is_some())
    {
        return Ok(value.clone());
    }
    let context = ZVal::from(init_span_context_object(
        span_context_class,
        SpanContext::empty_context(),
        None,
    )?);
    let span = new_non_recording_span(context)?;
    span_base
        .as_class_entry()
        .set_static_property("invalidSpan", span.clone());
    Ok(span)
}

fn context_is_local_root(context: &mut ZObj, span_key: &ZVal) -> phper::Result<bool> {
    let span = context.call("get", &mut [span_key.clone()])?;
    if span.as_z_obj().is_none() {
        return Ok(true);
    }
    let mut span = span;
    let mut span_context = span.expect_mut_z_obj()?.call("getContext", [])?;
    let span_context = span_context_from_object(span_context.expect_mut_z_obj()?)?;
    Ok(!span_context.is_valid() || span_context.is_remote())
}

fn span_from_context(
    context: &mut ZObj,
    span_key: &ZVal,
    span_base: &SpanBaseClass,
    span_context_class: &SpanContextClass,
    trace_state_class: &TraceStateClass,
) -> phper::Result<ZVal> {
    let span = context.call("get", &mut [span_key.clone()])?;
    if span.as_z_obj().is_some() {
        return Ok(span);
    }
    if let Some(native) = native_context_from_object(context) {
        let span_context = native.span().span_context().clone();
        if span_context.is_valid() {
            let context = ZVal::from(init_span_context_object(
                span_context_class,
                span_context,
                Some(trace_state_class),
            )?);
            return new_non_recording_span(context);
        }
    }
    invalid_span(span_base, span_context_class)
}

pub fn make_span_base_class(
    context_class: ContextClass,
    key_class: ContextKeyClass,
    keys_class: ContextKeysClass,
    span_context_class: SpanContextClass,
    trace_state_class: TraceStateClass,
    span_interface: Interface,
) -> ClassEntity<()> {
    let mut class = ClassEntity::new(SPAN_CLASS_NAME);
    class.set_abstract();
    class.implements(span_interface);
    class.add_static_property("invalidSpan", Visibility::Private, ());
    let span_base = class.bound_class();

    let from_key_class = key_class.clone();
    let from_keys_class = keys_class.clone();
    let from_span_base = span_base.clone();
    let from_span_context = span_context_class.clone();
    let from_trace_state = trace_state_class.clone();
    class
        .add_static_method("fromContext", Visibility::Public, move |arguments| {
            let span_key = get_or_create_context_key(
                &from_keys_class,
                &from_key_class,
                "span",
                "opentelemetry-trace-span-key",
            )?;
            span_from_context(
                util::arg_mut(arguments, 0)?.expect_mut_z_obj()?,
                &span_key,
                &from_span_base,
                &from_span_context,
                &from_trace_state,
            )
        })
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string())),
        )
        .return_type(span_return())
        .set_final();

    let current_context_class = context_class.clone();
    let current_key_class = key_class.clone();
    let current_keys_class = keys_class.clone();
    let current_span_base = span_base.clone();
    let current_span_context = span_context_class.clone();
    let current_trace_state = trace_state_class.clone();
    class
        .add_static_method("getCurrent", Visibility::Public, move |_| {
            let mut context = crate::context::context_class::current_context_value(&current_context_class)?;
            let span_key = get_or_create_context_key(
                &current_keys_class,
                &current_key_class,
                "span",
                "opentelemetry-trace-span-key",
            )?;
            span_from_context(
                context.expect_mut_z_obj()?,
                &span_key,
                &current_span_base,
                &current_span_context,
                &current_trace_state,
            )
        })
        .return_type(span_return())
        .set_final();

    let invalid_span_base = span_base.clone();
    let invalid_span_context = span_context_class.clone();
    class
        .add_static_method("getInvalid", Visibility::Public, move |_| {
            invalid_span(&invalid_span_base, &invalid_span_context)
        })
        .return_type(span_return())
        .set_final();

    let wrap_span_base = span_base.clone();
    let wrap_span_context = span_context_class.clone();
    class
        .add_static_method("wrap", Visibility::Public, move |arguments| {
            let context = util::arg_mut(arguments, 0)?;
            let valid = context
                .expect_mut_z_obj()?
                .call("isValid", [])?
                .as_bool()
                .unwrap_or(false);
            if !valid {
                return invalid_span(&wrap_span_base, &wrap_span_context);
            }
            new_non_recording_span(context.clone())
        })
        .argument(
            Argument::new("spanContext").with_type_hint(ArgumentTypeHint::ClassEntry(
                SPAN_CONTEXT_INTERFACE.to_string(),
            )),
        )
        .return_type(span_return())
        .set_final();

    let activate_context_class = context_class.clone();
    class
        .add_method("activate", Visibility::Public, move |this, _| {
            let mut context = crate::context::context_class::current_context_value(&activate_context_class)?;
            context.expect_mut_z_obj()?.call(
                "withContextValue",
                &mut [ZVal::from(this.to_ref_owned())],
            )?
            .expect_mut_z_obj()?
            .call("activate", [])
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SCOPE_INTERFACE.to_string(),
        )))
        .set_final();

    class
        .add_method("storeInContext", Visibility::Public, move |this, arguments| {
            let context_value = util::arg(arguments, 0)?.clone();
            let context_obj = util::arg_mut(arguments, 0)?.expect_mut_z_obj()?;
            let span_key = get_or_create_context_key(
                &keys_class,
                &key_class,
                "span",
                "opentelemetry-trace-span-key",
            )?;
            let is_local_root = context_is_local_root(context_obj, &span_key)?;
            let span_value = ZVal::from(this.to_ref_owned());

            if let Some(parent) = native_context_from_object(context_obj) {
                let mut span_context_value = this.call("getContext", [])?;
                let span_context =
                    span_context_from_object(span_context_value.expect_mut_z_obj()?)?;
                let otel = if span_context.is_valid() {
                    (**parent).clone().with_remote_span_context(span_context)
                } else {
                    (**parent).clone().with_span(NoopSpan::DEFAULT)
                };
                let mut next = parent.with_context(otel);
                next = next.with_value(
                    span_key.expect_z_obj()?,
                    &span_key,
                    &span_value,
                );
                if is_local_root {
                    let root_key = local_root_key(&key_class)?;
                    next = next.with_value(
                        root_key.expect_z_obj()?,
                        &root_key,
                        &span_value,
                    );
                }
                let context = Rc::new(next);
                return Ok::<_, phper::Error>(ZVal::from(init_context_object(
                    &context_class,
                    context,
                    None,
                )?));
            }

            let mut context = context_value;
            if is_local_root {
                let root_key = local_root_key(&key_class)?;
                context = context.expect_mut_z_obj()?.call(
                    "with",
                    &mut [root_key, span_value.clone()],
                )?;
            }
            context.expect_mut_z_obj()?.call(
                "with",
                &mut [span_key, span_value],
            )
        })
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string())),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_INTERFACE.to_string(),
        )));

    class
}

pub fn make_span_class(
    parent: SpanBaseClass,
    span_context_class: SpanContextClass,
    trace_state_class: TraceStateClass,
) -> ClassEntity<Option<SdkSpan>> {
    let mut class =
        ClassEntity::<Option<SdkSpan>>::new_with_default_state_constructor(NATIVE_SPAN_CLASS_NAME);
    class.extends(parent);
    class.add_property("context_id", Visibility::Private, 0i64);
    class.add_method("__construct", Visibility::Private, |_, _| Ok::<_, Infallible>(()));

    class
        .add_method("isRecording", Visibility::Public, |this, _| {
            let recording = if let Some(span) = this.as_state().as_ref() {
                span.is_recording()
            } else if let Some(context) = storage::get_context_instance(get_instance_id(this)) {
                context.span().is_recording()
            } else {
                false
            };
            Ok::<_, Infallible>(recording)
        })
        .return_type(ReturnType::new(ReturnTypeHint::Bool));

    class
        .add_method("end", Visibility::Public, |this, arguments| -> phper::Result<()> {
            let at = timestamp(arguments.first());
            let mut ended_context = None;
            if let Some(span) = this.as_mut_state().as_mut() {
                ended_context = Some(span.span_context().clone());
                if let Some(at) = at {
                    span.end_with_timestamp(at);
                } else {
                    span.end();
                }
            } else if let Some(context) = storage::get_context_instance(get_instance_id(this)) {
                if let Some(at) = at {
                    context.span().end_with_timestamp(at);
                } else {
                    context.span().end();
                }
            }
            if let Some(span_context) = ended_context {
                storage::remove_detached_span_context(&span_context);
            }
            Ok(())
        })
        .argument(
            Argument::new("endEpochNanos")
                .with_type_hint(ArgumentTypeHint::Int)
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    class
        .add_method("setStatus", Visibility::Public, |this, arguments| {
            let code = util::arg(arguments, 0)?.expect_z_str()?.to_str()?;
            let description = arguments
                .get(1)
                .and_then(ZVal::as_z_str)
                .and_then(|value| value.to_str().ok())
                .map_or(Cow::Borrowed(""), |value| Cow::Owned(value.to_string()));
            let status = match code {
                "Ok" => Status::Ok,
                "Error" => Status::Error { description },
                _ => Status::Unset,
            };
            if let Some(span) = this.as_mut_state().as_mut() {
                span.set_status(status);
            } else if let Some(context) = storage::get_context_instance(get_instance_id(this)) {
                context.span().set_status(status);
            }
            Ok::<_, phper::Error>(this.to_ref_owned())
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
        .add_method("setAttribute", Visibility::Public, |this, arguments| {
            let key = util::attribute_key(util::arg(arguments, 0)?.expect_z_str()?.to_str()?);
            if let Some(attribute) = util::zval_to_key_value(
                util::AttributeDestination::Span,
                key,
                util::arg(arguments, 1)?,
            ) {
                if let Some(span) = this.as_mut_state().as_mut() {
                    span.set_attribute(attribute);
                } else if let Some(context) = storage::get_context_instance(get_instance_id(this)) {
                    context.span().set_attribute(attribute);
                }
            }
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .argument(
            Argument::new("value").with_type_hint(ArgumentTypeHint::Union(vec![
                ArgumentTypeHint::Bool,
                ArgumentTypeHint::Int,
                ArgumentTypeHint::Float,
                ArgumentTypeHint::String,
                ArgumentTypeHint::Array,
                ArgumentTypeHint::Null,
            ])),
        )
        .return_type(span_return());

    class
        .add_method("setAttributes", Visibility::Public, |this, arguments| {
            let attributes = iterable_attributes(
                util::arg(arguments, 0)?,
                util::AttributeDestination::Span,
            )?;
            if let Some(span) = this.as_mut_state().as_mut() {
                span.set_attributes(attributes);
            } else if let Some(context) = storage::get_context_instance(get_instance_id(this)) {
                context.span().set_attributes(attributes);
            }
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("attributes").with_type_hint(ArgumentTypeHint::Iterable))
        .return_type(span_return());

    class
        .add_method("addLink", Visibility::Public, |this, arguments| {
            let span_context = span_context_from_object(
                util::arg_mut(arguments, 0)?.expect_mut_z_obj()?,
            )?;
            let attributes = optional_iterable_attributes(
                arguments.get(1),
                util::AttributeDestination::Link,
            )?;
            if let Some(span) = this.as_mut_state().as_mut() {
                span.add_link(span_context, attributes);
            } else if let Some(context) = storage::get_context_instance(get_instance_id(this)) {
                context.span().add_link(span_context, attributes);
            }
            Ok::<_, phper::Error>(this.to_ref_owned())
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
        .add_method("addEvent", Visibility::Public, |this, arguments| {
            let name = util::arg(arguments, 0)?.expect_z_str()?.to_str()?.to_string();
            let attributes = optional_iterable_attributes(
                arguments.get(1),
                util::AttributeDestination::Event,
            )?;
            let at = timestamp(arguments.get(2));
            if let Some(span) = this.as_mut_state().as_mut() {
                if let Some(at) = at {
                    span.add_event_with_timestamp(name, at, attributes);
                } else {
                    span.add_event(name, attributes);
                }
            } else if let Some(context) = storage::get_context_instance(get_instance_id(this)) {
                if let Some(at) = at {
                    context.span().add_event_with_timestamp(name, at, attributes);
                } else {
                    context.span().add_event(name, attributes);
                }
            }
            Ok::<_, phper::Error>(this.to_ref_owned())
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
        .add_method("recordException", Visibility::Public, |this, arguments| {
            let exception = util::arg_mut(arguments, 0)?.expect_mut_z_obj()?;
            let mut attributes = crate::error::php_exception_to_attributes(exception);
            attributes.extend(optional_iterable_attributes(
                arguments.get(1),
                util::AttributeDestination::Event,
            )?);
            let attributes = util::limit_key_values(
                attributes,
                util::AttributeDestination::Event,
            );
            if let Some(span) = this.as_mut_state().as_mut() {
                span.add_event("exception", attributes);
            } else if let Some(context) = storage::get_context_instance(get_instance_id(this)) {
                context.span().add_event("exception", attributes);
            }
            Ok::<_, phper::Error>(this.to_ref_owned())
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
        .add_method("updateName", Visibility::Public, |this, arguments| {
            let name = util::arg(arguments, 0)?.expect_z_str()?.to_str()?.to_string();
            if let Some(span) = this.as_mut_state().as_mut() {
                span.update_name(name);
            } else if let Some(context) = storage::get_context_instance(get_instance_id(this)) {
                context.span().update_name(name);
            }
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("name").with_type_hint(ArgumentTypeHint::String))
        .return_type(span_return());

    class
        .add_method("getContext", Visibility::Public, move |this, _| {
            let span_context = if let Some(span) = this.as_state().as_ref() {
                span.span_context().clone()
            } else if let Some(context) = storage::get_context_instance(get_instance_id(this)) {
                context.span().span_context().clone()
            } else {
                SpanContext::empty_context()
            };
            init_span_context_object(
                &span_context_class,
                span_context,
                Some(&trace_state_class),
            )
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_CONTEXT_INTERFACE.to_string(),
        )));

    class
}

pub fn context_bound_span(span_class: &SpanClass, instance_id: u64) -> phper::Result<ZVal> {
    let mut object = span_class.init_object()?;
    *object.as_mut_state() = None;
    object.set_property("context_id", instance_id as i64);
    Ok(ZVal::from(object))
}
