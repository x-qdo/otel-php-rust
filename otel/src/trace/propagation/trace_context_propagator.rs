use crate::context::{
    context::ContextClass,
    context_key::{ContextKeyClass, ContextKeysClass, get_or_create_context_key},
    propagation::array_access_getter_setter::{
        ArrayAccessGetterSetterClass, array_access_getter_setter_instance,
    },
};
use crate::trace::{
    span::{new_non_recording_span, otel_context_from_php},
    span_context::{SpanContextClass, init_span_context_object},
    trace_state::TraceStateClass,
};
use opentelemetry::trace::TraceContextExt;
use phper::{
    arrays::ZArray,
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};
use std::convert::Infallible;
use std::rc::Rc;

pub type TraceContextPropagatorClass = StateClass<()>;

pub fn make_trace_context_propagator_class(
    text_map_propagator_interface: Interface,
    context_class: ContextClass,
    key_class: ContextKeyClass,
    keys_class: ContextKeysClass,
    span_context_class: SpanContextClass,
    trace_state_class: TraceStateClass,
    array_access_class: ArrayAccessGetterSetterClass,
) -> ClassEntity<()> {
    let mut class = ClassEntity::<()>::new_with_default_state_constructor(
        r"OpenTelemetry\API\Trace\Propagation\TraceContextPropagator",
    );
    class.set_final();
    class.state_cloner(Clone::clone);
    class.implements(text_map_propagator_interface);
    class.add_constant("TRACEPARENT", "traceparent");
    class.add_constant("TRACESTATE", "tracestate");
    class.add_static_property("instance", Visibility::Private, ());
    let propagator_class = class.bound_class();

    let instance_class = propagator_class.clone();
    let instance_owner = propagator_class.clone();
    class
        .add_static_method("getInstance", Visibility::Public, move |_| {
            if let Some(value) = instance_owner
                .as_class_entry()
                .get_static_property("instance")
                .filter(|value| value.as_z_obj().is_some())
            {
                return Ok::<_, phper::Error>(value.clone());
            }
            let value = ZVal::from(instance_class.init_object()?);
            instance_owner
                .as_class_entry()
                .set_static_property("instance", value.clone());
            Ok(value)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            "self".to_string(),
        )));

    class
        .add_method("fields", Visibility::Public, |_, _| {
            let mut fields = ZArray::new();
            fields.insert((), "traceparent");
            fields.insert((), "tracestate");
            Ok::<_, Infallible>(fields)
        })
        .return_type(ReturnType::new(ReturnTypeHint::Array));

    let inject_context_class = context_class.clone();
    let inject_key_class = key_class.clone();
    let inject_keys_class = keys_class.clone();
    let inject_array_access_class = array_access_class.clone();
    class
        .add_method(
            "inject",
            Visibility::Public,
            move |_, arguments| -> phper::Result<()> {
                let mut context_value = if let Some(context) =
                    arguments.get(2).filter(|value| value.as_z_obj().is_some())
                {
                    context.clone()
                } else {
                    crate::context::context::current_context_value(&inject_context_class)?
                };
                let context = otel_context_from_php(
                    context_value.expect_mut_z_obj()?,
                    &inject_key_class,
                    &inject_keys_class,
                )?;

                // Carrier gymnastics (PHP array passed by ref)
                let mut out_map = std::collections::HashMap::<String, String>::new();

                // Use global propagator to inject
                opentelemetry::global::get_text_map_propagator(|prop| {
                    prop.inject_context(&context, &mut out_map);
                });

                let mut setter = match arguments.get(1).filter(|value| value.as_z_obj().is_some()) {
                    Some(setter) => setter.clone(),
                    None => array_access_getter_setter_instance(&inject_array_access_class)?,
                };
                for (key, value) in out_map {
                    setter.expect_mut_z_obj()?.call(
                        "set",
                        &mut [
                            crate::util::arg(arguments, 0)?.clone(),
                            ZVal::from(key),
                            ZVal::from(value),
                        ],
                    )?;
                }
                Ok::<_, phper::Error>(())
            },
        )
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
                .with_default_value("NULL")
                .allow_null(),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    let context_ce = context_class;
    class
        .add_method("extract", Visibility::Public, move |_, arguments| {
            let mut map = std::collections::HashMap::<String, String>::new();
            let mut getter = match arguments.get(1).filter(|value| value.as_z_obj().is_some()) {
                Some(getter) => getter.clone(),
                None => array_access_getter_setter_instance(&array_access_class)?,
            };
            for key in ["traceparent", "tracestate"] {
                let value = getter.expect_mut_z_obj()?.call(
                    "get",
                    &mut [crate::util::arg(arguments, 0)?.clone(), ZVal::from(key)],
                )?;
                if let Some(value) = value.as_z_str() {
                    map.insert(key.to_string(), value.to_str()?.to_string());
                }
            }

            // Parent context (optional)
            let parent_value = if let Some(context) =
                arguments.get(2).filter(|value| value.as_z_obj().is_some())
            {
                context.clone()
            } else {
                crate::context::context::current_context_value(&context_ce)?
            };
            let parent_native = parent_value
                .as_z_obj()
                .and_then(crate::context::context::native_context_from_object);
            let mut parent_for_context = parent_value.clone();
            let parent_cx = otel_context_from_php(
                parent_for_context.expect_mut_z_obj()?,
                &key_class,
                &keys_class,
            )?;

            // Extract new context from headers
            let new_cx = opentelemetry::global::get_text_map_propagator(|prop| {
                prop.extract_with_context(&parent_cx, &map)
            });
            let span_context = new_cx.span().span_context().clone();
            if !span_context.is_valid() {
                return Ok::<_, phper::Error>(parent_value);
            }
            let php_span_context = ZVal::from(init_span_context_object(
                &span_context_class,
                span_context,
                Some(&trace_state_class),
            )?);
            let span = new_non_recording_span(php_span_context)?;
            let Some(parent_native) = parent_native else {
                let mut parent = parent_value;
                return parent
                    .expect_mut_z_obj()?
                    .call("withContextValue", &mut [span]);
            };
            let span_key = get_or_create_context_key(
                &keys_class,
                &key_class,
                "span",
                "opentelemetry-trace-span-key",
            )?;
            let native = parent_native.with_context(new_cx);
            let native = native.with_value(span_key.expect_z_obj()?, &span_key, &span);
            let new_cx = Rc::new(native);
            // Wrap in PHP context object
            let mut obj = context_ce.init_object()?;
            *obj.as_mut_state() = Some(new_cx);
            obj.set_property("context_id", 0i64);
            Ok::<_, phper::Error>(ZVal::from(obj))
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
                .with_default_value("NULL")
                .allow_null(),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\Context\ContextInterface".to_string(),
        )));

    class
}
