use crate::{
    context::{
        context::{ContextClass, get_instance_id},
        context_key::{ContextKeyClass, ContextKeysClass, get_or_create_context_key},
        native_context::NativeContext,
        storage,
    },
    trace::{
        non_recording_span::NonRecordingSpanClass,
        span::{SpanClass, context_bound_span},
        span_context::span_context_from_object,
        span_interface::SPAN_INTERFACE,
    },
};
use phper::{
    classes::{ClassEntity, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};
use std::{cell::RefCell, rc::Rc};

const LOCAL_ROOT_SPAN_CLASS_NAME: &str = r"OpenTelemetry\API\Trace\LocalRootSpan";
const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";
const CONTEXT_KEY_INTERFACE: &str = r"OpenTelemetry\Context\ContextKeyInterface";

thread_local! {
    static LOCAL_ROOT_SPAN_ID: RefCell<Option<u64>> = const { RefCell::new(None) };
    static LOCAL_ROOT_KEY: RefCell<Option<ZVal>> = const { RefCell::new(None) };
}

pub fn local_root_key(key_class: &ContextKeyClass) -> phper::Result<ZVal> {
    LOCAL_ROOT_KEY.with(|slot| {
        if let Some(key) = slot.borrow().as_ref() {
            return Ok(key.clone());
        }
        let mut object = key_class.init_object()?;
        *object.as_mut_state() = Some(LOCAL_ROOT_SPAN_CLASS_NAME.to_string());
        let value = ZVal::from(object);
        *slot.borrow_mut() = Some(value.clone());
        Ok(value)
    })
}

fn invalid_span(class: &NonRecordingSpanClass) -> phper::Result<ZVal> {
    Ok(ZVal::from(class.init_object()?))
}

fn from_context(
    context: &mut phper::objects::ZObj,
    key_class: &ContextKeyClass,
    span_class: &SpanClass,
    non_recording_class: &NonRecordingSpanClass,
) -> phper::Result<ZVal> {
    let key = local_root_key(key_class)?;
    let span = context.call("get", &mut [key])?;
    if span.as_z_obj().is_some() {
        return Ok(span);
    }
    let context_id = get_instance_id(context);
    if let (Some(context_id), Some(local_root_id)) =
        (context_id, get_local_root_span_instance_id())
        && context_id == local_root_id
    {
        return context_bound_span(span_class, context_id);
    }
    invalid_span(non_recording_class)
}

pub fn make_local_root_span_class(
    context_class: ContextClass,
    key_class: ContextKeyClass,
    keys_class: ContextKeysClass,
    span_class: SpanClass,
    non_recording_span_class: NonRecordingSpanClass,
) -> ClassEntity<()> {
    let mut class = ClassEntity::<()>::new_with_default_state_constructor(
        LOCAL_ROOT_SPAN_CLASS_NAME,
    );
    class.state_cloner(Clone::clone);

    let current_key = key_class.clone();
    let current_span = span_class.clone();
    let current_non_recording = non_recording_span_class.clone();
    class
        .add_static_method("current", Visibility::Public, move |_| {
            let mut context = crate::context::context::current_context_value(&context_class)?;
            from_context(
                context.expect_mut_z_obj()?,
                &current_key,
                &current_span,
                &current_non_recording,
            )
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_INTERFACE.to_string(),
        )));

    let from_key = key_class.clone();
    let from_span = span_class.clone();
    let from_non_recording = non_recording_span_class.clone();
    class
        .add_static_method("fromContext", Visibility::Public, move |arguments| {
            from_context(
                crate::util::arg_mut(arguments, 0)?.expect_mut_z_obj()?,
                &from_key,
                &from_span,
                &from_non_recording,
            )
        })
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string())),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_INTERFACE.to_string(),
        )));

    let store_key = key_class.clone();
    class
        .add_static_method("store", Visibility::Public, move |arguments| {
            let key = local_root_key(&store_key)?;
            let span = crate::util::arg(arguments, 1)?.clone();
            crate::util::arg_mut(arguments, 0)?
                .expect_mut_z_obj()?
                .call("with", &mut [key, span])
        })
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string())),
        )
        .argument(
            Argument::new("span")
                .with_type_hint(ArgumentTypeHint::ClassEntry(SPAN_INTERFACE.to_string())),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_INTERFACE.to_string(),
        )));

    let key_method_class = key_class.clone();
    class
        .add_static_method("key", Visibility::Public, move |_| {
            local_root_key(&key_method_class)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_KEY_INTERFACE.to_string(),
        )));

    class
        .add_static_method("isLocalRoot", Visibility::Public, move |arguments| {
            let span_key = get_or_create_context_key(
                &keys_class,
                &key_class,
                "span",
                "opentelemetry-trace-span-key",
            )?;
            let span = crate::util::arg_mut(arguments, 0)?
                .expect_mut_z_obj()?
                .call("get", &mut [span_key])?;
            if span.as_z_obj().is_none() {
                return Ok::<_, phper::Error>(true);
            }
            let mut span = span;
            let mut span_context = span.expect_mut_z_obj()?.call("getContext", [])?;
            let span_context =
                span_context_from_object(span_context.expect_mut_z_obj()?)?;
            Ok(!span_context.is_valid() || span_context.is_remote())
        })
        .argument(
            Argument::new("parentContext")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string())),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Bool));

    class
}

pub fn store_local_root_span(context_id: Option<u64>) {
    LOCAL_ROOT_SPAN_ID.with(|cell| *cell.borrow_mut() = context_id);
}

pub fn get_local_root_span_instance_id() -> Option<u64> {
    LOCAL_ROOT_SPAN_ID.with(|cell| *cell.borrow())
}

pub fn get_local_root_span_context() -> Option<Rc<NativeContext>> {
    LOCAL_ROOT_SPAN_ID.with(|cell| {
        cell.borrow()
            .and_then(|context_id| storage::get_context_instance(Some(context_id)))
    })
}

pub fn maybe_remove_local_root_span(context_id: Option<u64>) {
    LOCAL_ROOT_SPAN_ID.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        match (context_id, *borrowed) {
            (Some(id), Some(current_id)) if id == current_id => {
                *borrowed = None;
                storage::maybe_remove_context_instance(Some(id));
            }
            (None, Some(current_id)) => {
                *borrowed = None;
                storage::maybe_remove_context_instance(Some(current_id));
            }
            _ => {}
        }
    });
}

pub fn clear_request_state() {
    LOCAL_ROOT_SPAN_ID.with(|cell| *cell.borrow_mut() = None);
    LOCAL_ROOT_KEY.with(|cell| *cell.borrow_mut() = None);
}
