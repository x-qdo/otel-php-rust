use crate::{
    context::{
        context::{ContextClass, get_instance_id},
        scope::ScopeClass,
        storage,
    },
    trace::{local_root_span::store_local_root_span, span_context::SpanContextClass},
    util,
};
use opentelemetry::{
    Context,
    trace::{Span, SpanContext, Status, TraceContextExt},
};
use opentelemetry_sdk::trace::Span as SdkSpan;
use phper::{
    alloc::ToRefOwned,
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    objects::{StateObject, ZObj},
    types::ReturnTypeHint,
};
use std::{borrow::Cow, convert::Infallible, sync::Arc};

const SPAN_CLASS_NAME: &str = r"OpenTelemetry\API\Trace\Span";

pub type SpanClass = StateClass<Option<SdkSpan>>;

pub fn make_span_class(
    scope_class: ScopeClass,
    span_context_class: SpanContextClass,
    context_class: ContextClass,
    span_interface: &Interface,
) -> ClassEntity<Option<SdkSpan>> {
    let mut class =
        ClassEntity::<Option<SdkSpan>>::new_with_default_state_constructor(SPAN_CLASS_NAME);
    let span_class = class.bound_class();

    class.implements(span_interface.clone());

    class.add_property("context_id", Visibility::Private, 0i64);
    class.add_property("is_local_root", Visibility::Private, false);

    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method("isRecording", Visibility::Public, |this, _| {
            let is_recording = if let Some(span) = this.as_state().as_ref() {
                span.is_recording()
            } else if let Some(ctx) = storage::get_context_instance(get_instance_id(this)) {
                ctx.span().is_recording()
            } else {
                false
            };

            Ok::<_, phper::Error>(is_recording)
        })
        .return_type(ReturnType::new(ReturnTypeHint::Bool));

    class.add_method("end", Visibility::Public, |this, _| -> phper::Result<()> {
        if let Some(span) = this.as_mut_state().as_mut() {
            tracing::debug!("Span::Ending Span (SdkSpan)");
            span.end();
        } else {
            let instance_id = get_instance_id(this);
            {
                //in own block to ensure reference dropped before remove
                if let Some(ctx) = storage::get_context_instance(instance_id) {
                    tracing::debug!("Span::Ending Span (SpanRef)");
                    ctx.span().end();
                }
            }
            if let Some(id) = instance_id {
                storage::remove_context_instance(id);
            }
        }

        Ok(())
    });

    class
        .add_method("setStatus", Visibility::Public, |this, arguments| {
            let status = match arguments[0].expect_z_str()?.to_str() {
                Ok(s) => s.to_string(),
                Err(_) => return Ok(this.to_ref_owned()), // Ignore invalid UTF-8 input
            };
            let description: Cow<'static, str> = arguments
                .get(1)
                .map(|d| d.expect_z_str())
                .transpose()?
                .map(|d| match d.to_str() {
                    Ok(s) => Cow::Owned(s.to_string()),
                    Err(_) => Cow::Borrowed(""),
                })
                .unwrap_or(Cow::Borrowed(""));
            let status_code = match status.as_str() {
                "Ok" => Status::Ok,
                "Unset" => Status::Unset,
                "Error" => Status::Error { description },
                _ => Status::Unset,
            };

            if let Some(span) = this.as_mut_state().as_mut() {
                span.set_status(status_code);
            } else if let Some(ctx) = storage::get_context_instance(get_instance_id(this)) {
                ctx.span().set_status(status_code);
            }

            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("code"))
        .argument(Argument::new("description").optional());

    class.add_method("setAttribute", Visibility::Public, |this, arguments| {
        let key = util::attribute_key(arguments[0].expect_z_str()?.to_str()?);
        if let Some(kv) = util::zval_to_key_value(key, &arguments[1]) {
            if let Some(span) = this.as_mut_state().as_mut() {
                span.set_attribute(kv);
            } else if let Some(ctx) = storage::get_context_instance(get_instance_id(this)) {
                ctx.span().set_attribute(kv);
            }
        }

        Ok::<_, phper::Error>(this.to_ref_owned())
    });

    class.add_method("setAttributes", Visibility::Public, |this, arguments| {
        let attributes = util::zval_arr_to_key_value_vec(arguments[0].expect_z_arr()?);
        if let Some(span) = this.as_mut_state().as_mut() {
            span.set_attributes(attributes);
        } else if let Some(ctx) = storage::get_context_instance(get_instance_id(this)) {
            ctx.span().set_attributes(attributes);
        }

        Ok::<_, phper::Error>(this.to_ref_owned())
    });

    class.add_method("updateName", Visibility::Public, |this, arguments| {
        tracing::debug!("Span::updateName");
        let name = arguments[0].expect_z_str()?.to_str()?.to_string();

        if let Some(span) = this.as_mut_state().as_mut() {
            tracing::debug!("Span::updateName (SdkSpan)");
            span.update_name(name);
        } else if let Some(ctx) = storage::get_context_instance(get_instance_id(this)) {
            tracing::debug!("Span::updateName (SpanRef)");
            ctx.span().update_name(name);
        }
        Ok::<_, phper::Error>(this.to_ref_owned())
    });

    class.add_method("recordException", Visibility::Public, |this, arguments| {
        let exception = arguments[0].expect_mut_z_obj()?;
        let attributes = crate::error::php_exception_to_attributes(exception);
        if let Some(span) = this.as_mut_state().as_mut() {
            span.add_event("exception", attributes);
        } else if let Some(ctx) = storage::get_context_instance(get_instance_id(this)) {
            ctx.span().add_event("exception", attributes);
        }
        Ok::<_, phper::Error>(this.to_ref_owned())
    });

    class.add_method("addLink", Visibility::Public, |this, arguments| {
        let span_context = {
            let span_context_obj: &mut ZObj = arguments[0].expect_mut_z_obj()?;
            let state_obj = unsafe { span_context_obj.as_state_obj::<Option<SpanContext>>() };
            match state_obj.as_state() {
                Some(value) => value.clone(),
                None => return Err(phper::Error::boxed("Invalid SpanContext object")),
            }
        };

        let attributes = arguments
            .get(1)
            .and_then(|argument| argument.as_z_arr())
            .map(util::zval_arr_to_key_value_vec)
            .unwrap_or_default();
        if let Some(span) = this.as_mut_state().as_mut() {
            span.add_link(span_context, attributes);
        } else if let Some(ctx) = storage::get_context_instance(get_instance_id(this)) {
            ctx.span().add_link(span_context, attributes);
        }

        Ok::<_, phper::Error>(this.to_ref_owned())
    });

    class.add_method("addEvent", Visibility::Public, |this, arguments| {
        let event_name = arguments[0].expect_z_str()?.to_str()?.to_string();
        let attributes = arguments
            .get(1)
            .and_then(|attrs| attrs.as_z_arr())
            .map(util::zval_arr_to_key_value_vec)
            .unwrap_or_default();

        if let Some(span) = this.as_mut_state().as_mut() {
            span.add_event(event_name, attributes);
        } else if let Some(ctx) = storage::get_context_instance(get_instance_id(this)) {
            ctx.span().add_event(event_name, attributes);
        }

        Ok::<_, phper::Error>(this.to_ref_owned())
    });

    class.add_method("getContext", Visibility::Public, move |this, _| {
        let mut object = span_context_class.init_object()?;
        if let Some(sdk_span) = this.as_state().as_ref() {
            *object.as_mut_state() = Some(sdk_span.span_context().clone());
        } else if let Some(ctx) = storage::get_context_instance(get_instance_id(this)) {
            *object.as_mut_state() = Some(ctx.span().span_context().clone());
        }
        Ok::<_, phper::Error>(object)
    });

    class.add_static_method("getCurrent", Visibility::Public, {
        let span_class = span_class.clone();
        move |_| current_span_object(&span_class)
    });

    class.add_method("activate", Visibility::Public, {
        let scope_ce = scope_class.clone();
        move |this, _arguments| {
            let existing_id = get_instance_id(this);
            let instance_id = if let Some(span) = this.as_mut_state().take() {
                let is_local_root = !storage::current_context().span().span_context().is_valid();
                let ctx = Context::current_with_span(span);
                let instance_id = storage::store_context_instance(Arc::new(ctx));
                this.set_property("context_id", instance_id.unwrap_or(0) as i64);
                if is_local_root {
                    this.set_property("is_local_root", true);
                    store_local_root_span(instance_id);
                }
                instance_id
            } else if storage::get_context_instance(existing_id).is_some() {
                existing_id
            } else {
                None
            };

            if instance_id.is_some() {
                storage::attach_context(instance_id).map_err(phper::Error::boxed)?;
            }

            let mut object = scope_ce.init_object()?;
            object.as_mut_state().context = storage::get_context_instance(instance_id);
            object.set_property("context_id", instance_id.unwrap_or(0) as i64);
            Ok::<_, phper::Error>(object)
        }
    });

    class.add_method(
        "storeInContext",
        Visibility::Public,
        move |this, arguments| {
            let context_obj: &mut ZObj = arguments[0].expect_mut_z_obj()?;
            let context_id = get_instance_id(context_obj);
            let arc_ctx = if let Some(span) = this.as_mut_state().take() {
                let context = storage::resolve_context(context_id);
                Arc::new(context.with_span(span))
            } else if let Some(context) = storage::get_context_instance(get_instance_id(this)) {
                context
            } else {
                storage::resolve_context(context_id)
            };
            let instance_id = storage::store_context_instance(arc_ctx.clone());

            let mut object = context_class.init_object()?;
            *object.as_mut_state() = Some(arc_ctx);
            object.set_property("context_id", instance_id.unwrap_or(0) as i64);

            Ok::<_, phper::Error>(object)
        },
    ); //argument ContextInterface, return ContextInterface

    let span_class_clone = class.bound_class();
    class.add_static_method("fromContext", Visibility::Public, move |arguments| {
        span_object_from_context(&span_class_clone, arguments[0].expect_mut_z_obj()?)
    });

    class
}

/// `Span::getCurrent()`: a span object bound to the current context.
pub fn current_span_object(span_class: &SpanClass) -> phper::Result<StateObject<Option<SdkSpan>>> {
    let ctx = storage::current_context();
    let instance_id = storage::store_context_instance(ctx);

    let mut object = span_class.init_object()?;
    *object.as_mut_state() = None;
    object.set_property("context_id", instance_id.unwrap_or(0) as i64);
    Ok(object)
}

/// `Span::fromContext($context)`: a span object bound to the span stored in `context`.
pub fn span_object_from_context(
    span_class: &SpanClass,
    context_obj: &mut ZObj,
) -> phper::Result<StateObject<Option<SdkSpan>>> {
    let existing_id = get_instance_id(context_obj);
    let instance_id = if storage::get_context_instance(existing_id).is_some() {
        existing_id
    } else {
        let state = unsafe { context_obj.as_state_obj::<Option<Arc<Context>>>() };
        state
            .as_state()
            .clone()
            .and_then(storage::store_context_instance)
    };
    let mut object = span_class.init_object()?;
    *object.as_mut_state() = None;
    object.set_property("context_id", instance_id.unwrap_or(0) as i64);
    Ok(object)
}
