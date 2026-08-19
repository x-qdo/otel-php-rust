use crate::trace::{
    span_context_interface::{SPAN_CONTEXT_INTERFACE, TRACE_STATE_INTERFACE},
    trace_state::{TraceStateClass, TraceStateState, otel_trace_state_from_header},
};
use opentelemetry::trace::{
    SpanContext, SpanId, TraceFlags as OtelTraceFlags, TraceId, TraceState as OtelTraceState,
};
use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    objects::{StateObject, ZObj},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};
use std::convert::Infallible;

pub const SPAN_CONTEXT_CLASS_NAME: &str = r"OpenTelemetry\API\Trace\SpanContext";

#[derive(Clone)]
pub struct SpanContextState {
    context: SpanContext,
    trace_state: Option<ZVal>,
    trace_flags: i64,
}

impl Default for SpanContextState {
    fn default() -> Self {
        Self {
            context: SpanContext::empty_context(),
            trace_state: None,
            trace_flags: 0,
        }
    }
}

pub type SpanContextClass = StateClass<SpanContextState>;

fn valid_hex_id(value: &str, length: usize) -> bool {
    value.len() == length
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}

fn decode_hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .filter_map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => 0,
            };
            let [high, low] = pair else {
                return None;
            };
            Some((digit(*high) << 4) | digit(*low))
        })
        .collect()
}

fn trace_state_from_value(value: Option<&ZVal>) -> phper::Result<(OtelTraceState, Option<ZVal>)> {
    let Some(value) = value.filter(|value| value.as_z_obj().is_some()) else {
        return Ok((OtelTraceState::default(), None));
    };
    let mut retained = value.clone();
    let header = retained
        .expect_mut_z_obj()?
        .call("toString", [])?
        .expect_z_str()?
        .to_str()?
        .to_string();
    Ok((otel_trace_state_from_header(&header), Some(value.clone())))
}

fn create_context(arguments: &mut [ZVal], remote: bool) -> phper::Result<SpanContextState> {
    let trace_id = crate::util::arg(arguments, 0)?.expect_z_str()?.to_str()?;
    let span_id = crate::util::arg(arguments, 1)?.expect_z_str()?.to_str()?;
    let valid = valid_hex_id(trace_id, 32) && valid_hex_id(span_id, 16);
    let trace_id = if valid {
        TraceId::from_hex(trace_id).unwrap_or(TraceId::INVALID)
    } else {
        TraceId::INVALID
    };
    let span_id = if valid {
        SpanId::from_hex(span_id).unwrap_or(SpanId::INVALID)
    } else {
        SpanId::INVALID
    };
    let flags = arguments.get(2).and_then(ZVal::as_long).unwrap_or(0);
    let (trace_state, php_trace_state) = trace_state_from_value(arguments.get(3))?;
    Ok(SpanContextState {
        context: SpanContext::new(
            trace_id,
            span_id,
            OtelTraceFlags::new(flags as u8),
            remote,
            trace_state,
        ),
        trace_state: php_trace_state,
        trace_flags: flags,
    })
}

pub fn span_context_from_object(object: &mut ZObj) -> phper::Result<SpanContext> {
    let is_native = object
        .get_class()
        .get_name()
        .to_str()
        .is_ok_and(|name| name == SPAN_CONTEXT_CLASS_NAME);
    if is_native {
        let state = unsafe { object.as_state_obj::<SpanContextState>() };
        return Ok(state.as_state().context.clone());
    }

    let trace_id = object.call("getTraceId", [])?.expect_z_str()?.to_str()?.to_string();
    let span_id = object.call("getSpanId", [])?.expect_z_str()?.to_str()?.to_string();
    let flags = object.call("getTraceFlags", [])?.as_long().unwrap_or(0) as u8;
    let remote = object.call("isRemote", [])?.as_bool().unwrap_or(false);
    let trace_state = object.call("getTraceState", [])?;
    let (trace_state, _) = trace_state_from_value(Some(&trace_state))?;
    let valid = valid_hex_id(&trace_id, 32) && valid_hex_id(&span_id, 16);
    Ok(SpanContext::new(
        if valid {
            TraceId::from_hex(&trace_id).unwrap_or(TraceId::INVALID)
        } else {
            TraceId::INVALID
        },
        if valid {
            SpanId::from_hex(&span_id).unwrap_or(SpanId::INVALID)
        } else {
            SpanId::INVALID
        },
        OtelTraceFlags::new(flags),
        remote,
        trace_state,
    ))
}

pub fn init_span_context_object(
    class: &SpanContextClass,
    context: SpanContext,
    trace_state_class: Option<&TraceStateClass>,
) -> phper::Result<StateObject<SpanContextState>> {
    let php_trace_state = if context.trace_state().header().is_empty() {
        None
    } else if let Some(trace_state_class) = trace_state_class {
        let mut object = trace_state_class.init_object()?;
        *object.as_mut_state() = TraceStateState::parse(&context.trace_state().header());
        Some(ZVal::from(object))
    } else {
        None
    };
    let mut object = class.init_object()?;
    *object.as_mut_state() = SpanContextState {
        trace_flags: context.trace_flags().to_u8() as i64,
        context,
        trace_state: php_trace_state,
    };
    Ok(object)
}

fn add_factory(
    class: &mut ClassEntity<SpanContextState>,
    name: &str,
    remote: bool,
    span_context_class: SpanContextClass,
) {
    class
        .add_static_method(name, Visibility::Public, move |arguments| {
            let state = create_context(arguments, remote)?;
            let mut object = span_context_class.init_object()?;
            *object.as_mut_state() = state;
            Ok::<_, phper::Error>(object)
        })
        .argument(Argument::new("traceId").with_type_hint(ArgumentTypeHint::String))
        .argument(Argument::new("spanId").with_type_hint(ArgumentTypeHint::String))
        .argument(
            Argument::new("traceFlags")
                .with_type_hint(ArgumentTypeHint::Int)
                .with_default_value(r"\OpenTelemetry\API\Trace\TraceFlags::DEFAULT"),
        )
        .argument(
            Argument::new("traceState")
                .with_type_hint(ArgumentTypeHint::ClassEntry(TRACE_STATE_INTERFACE.to_string()))
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_CONTEXT_INTERFACE.to_string(),
        )));
}

pub fn make_span_context_class(interface: Interface) -> ClassEntity<SpanContextState> {
    let mut class = ClassEntity::new_with_default_state_constructor(SPAN_CONTEXT_CLASS_NAME);
    class.set_final();
    class.state_cloner(Clone::clone);
    class.implements(interface);
    let span_context_class = class.bound_class();

    class.add_static_property("invalidContext", Visibility::Private, ());
    class.add_method("__construct", Visibility::Private, |_, _| Ok::<_, Infallible>(()));

    let invalid_class = span_context_class.clone();
    let invalid_owner = span_context_class.clone();
    class
        .add_static_method("getInvalid", Visibility::Public, move |_| {
            if let Some(value) = invalid_owner
                .as_class_entry()
                .get_static_property("invalidContext")
                .filter(|value| value.as_z_obj().is_some())
            {
                return Ok::<_, phper::Error>(value.clone());
            }
            let mut object = invalid_class.init_object()?;
            *object.as_mut_state() = SpanContextState::default();
            let value = ZVal::from(object);
            invalid_owner
                .as_class_entry()
                .set_static_property("invalidContext", value.clone());
            Ok(value)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_CONTEXT_INTERFACE.to_string(),
        )));

    add_factory(&mut class, "create", false, span_context_class.clone());
    add_factory(
        &mut class,
        "createFromRemoteParent",
        true,
        span_context_class,
    );

    class
        .add_method("getTraceId", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.as_state().context.trace_id().to_string())
        })
        .return_type(ReturnType::new(ReturnTypeHint::String));
    class
        .add_method("getTraceIdBinary", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(decode_hex(&this.as_state().context.trace_id().to_string()))
        })
        .return_type(ReturnType::new(ReturnTypeHint::String));
    class
        .add_method("getSpanId", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.as_state().context.span_id().to_string())
        })
        .return_type(ReturnType::new(ReturnTypeHint::String));
    class
        .add_method("getSpanIdBinary", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(decode_hex(&this.as_state().context.span_id().to_string()))
        })
        .return_type(ReturnType::new(ReturnTypeHint::String));
    class
        .add_method("getTraceFlags", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.as_state().trace_flags)
        })
        .return_type(ReturnType::new(ReturnTypeHint::Int));
    class
        .add_method("getTraceState", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.as_state().trace_state.clone())
        })
        .return_type(
            ReturnType::new(ReturnTypeHint::ClassEntry(TRACE_STATE_INTERFACE.to_string()))
                .allow_null(),
        );
    class
        .add_method("isValid", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.as_state().context.is_valid())
        })
        .return_type(ReturnType::new(ReturnTypeHint::Bool));
    class
        .add_method("isRemote", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.as_state().context.is_remote())
        })
        .return_type(ReturnType::new(ReturnTypeHint::Bool));
    class
        .add_method("isSampled", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.as_state().context.is_sampled())
        })
        .return_type(ReturnType::new(ReturnTypeHint::Bool));

    class
}
