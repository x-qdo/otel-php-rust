use crate::{
    context::context::{ContextClass, native_context_from_object},
    logs::severity::{otel_severity, severity_number},
    util::{self, AttributeDestination, AttributeLimits},
};
use opentelemetry::{
    Array, Context, Value,
    logs::{AnyValue, Severity},
    trace::{SpanId, TraceContextExt, TraceFlags, TraceId},
};
use phper::{
    alloc::ToRefOwned,
    arrays::IterKey,
    classes::{ClassEntity, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::{ZVal, ZValRef},
};
use std::{collections::HashMap, time::SystemTime};

pub const LOG_RECORD_CLASS_NAME: &str = r"OpenTelemetry\API\Logs\LogRecord";
const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";
const SEVERITY_ENUM: &str = r"OpenTelemetry\API\Logs\Severity";

#[derive(Clone, Copy, Debug)]
pub struct TraceContextData {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub trace_flags: TraceFlags,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum LogContext {
    #[default]
    Current,
    None,
    Explicit(Option<TraceContextData>),
}

#[derive(Clone, Debug, Default)]
pub struct LogRecordState {
    pub body: Option<AnyValue>,
    pub severity: Option<Severity>,
    pub severity_text: Option<String>,
    pub attributes: Vec<(String, AnyValue)>,
    pub event_name: Option<String>,
    pub timestamp: Option<SystemTime>,
    pub observed_timestamp: Option<SystemTime>,
    pub context: LogContext,
}

pub type LogRecordClass = StateClass<LogRecordState>;

pub fn any_value(value: &ZVal) -> Option<AnyValue> {
    any_value_with_limits(value, unlimited_any_value_limits(), 0)
}

fn attribute_any_value(value: &ZVal) -> Option<AnyValue> {
    any_value_with_limits(value, util::attribute_limits(AttributeDestination::Log), 0)
}

fn unlimited_any_value_limits() -> AttributeLimits {
    AttributeLimits {
        count: usize::MAX,
        key_length: usize::MAX,
        value_length: usize::MAX,
        array_length: usize::MAX,
    }
}

fn any_value_with_limits(value: &ZVal, limits: AttributeLimits, depth: usize) -> Option<AnyValue> {
    if depth >= 64 {
        return None;
    }
    match value.to_value().ok()? {
        ZValRef::Null => None,
        ZValRef::Bool(value) => Some(AnyValue::Boolean(value)),
        ZValRef::Long(value) => Some(AnyValue::Int(value)),
        ZValRef::Double(value) => Some(AnyValue::Double(value)),
        ZValRef::Str(value) => value.to_str().ok().map(|value| {
            AnyValue::String(util::truncate_string(value, limits.value_length).into())
        }),
        ZValRef::Arr(array) => {
            let has_string_key = array.iter().any(|(key, _)| matches!(key, IterKey::ZStr(_)));
            if has_string_key {
                let mut values = HashMap::new();
                for (key, value) in array.iter().take(limits.array_length) {
                    let key = match key {
                        IterKey::Index(index) => index.to_string(),
                        IterKey::ZStr(key) => key.to_str().ok()?.to_string(),
                    };
                    if let Some(value) = any_value_with_limits(value, limits, depth + 1) {
                        values.insert(key.into(), value);
                    }
                }
                Some(AnyValue::Map(Box::new(values)))
            } else {
                Some(AnyValue::ListAny(Box::new(
                    array
                        .iter()
                        .take(limits.array_length)
                        .filter_map(|(_, value)| any_value_with_limits(value, limits, depth + 1))
                        .collect(),
                )))
            }
        }
        ZValRef::Obj(object) => Some(AnyValue::String(
            util::truncate_string(&format!("{object:?}"), limits.value_length).into(),
        )),
        ZValRef::Res(resource) => Some(AnyValue::String(
            util::truncate_string(&format!("{resource:?}"), limits.value_length).into(),
        )),
        ZValRef::Ref(reference) => any_value_with_limits(reference.val(), limits, depth),
    }
}

pub fn otel_value_to_any(value: &Value) -> AnyValue {
    match value {
        Value::Bool(value) => AnyValue::Boolean(*value),
        Value::I64(value) => AnyValue::Int(*value),
        Value::F64(value) => AnyValue::Double(*value),
        Value::String(value) => AnyValue::String(value.clone()),
        Value::Array(Array::Bool(values)) => AnyValue::ListAny(Box::new(
            values.iter().copied().map(AnyValue::Boolean).collect(),
        )),
        Value::Array(Array::I64(values)) => AnyValue::ListAny(Box::new(
            values.iter().copied().map(AnyValue::Int).collect(),
        )),
        Value::Array(Array::F64(values)) => AnyValue::ListAny(Box::new(
            values.iter().copied().map(AnyValue::Double).collect(),
        )),
        Value::Array(Array::String(values)) => AnyValue::ListAny(Box::new(
            values.iter().cloned().map(AnyValue::String).collect(),
        )),
        _ => AnyValue::String(format!("{value:?}").into()),
    }
}

pub fn set_attribute(state: &mut LogRecordState, key: String, value: &ZVal) {
    let limits = util::attribute_limits(AttributeDestination::Log);
    if !util::valid_attribute_key(&key, limits) {
        return;
    }
    let Some(value) = attribute_any_value(value) else {
        return;
    };
    set_any_attribute(state, key, value, limits);
}

fn set_any_attribute(
    state: &mut LogRecordState,
    key: String,
    value: AnyValue,
    limits: AttributeLimits,
) {
    if let Some((_, existing)) = state
        .attributes
        .iter_mut()
        .find(|(existing, _)| existing == &key)
    {
        *existing = value;
    } else if state.attributes.len() < limits.count {
        state.attributes.push((key, value));
    }
}

pub fn set_otel_attribute(state: &mut LogRecordState, attribute: opentelemetry::KeyValue) {
    let limits = util::attribute_limits(AttributeDestination::Log);
    if !util::valid_attribute_key(attribute.key.as_str(), limits) {
        return;
    }
    set_any_attribute(
        state,
        attribute.key.as_str().to_string(),
        otel_value_to_any(&attribute.value),
        limits,
    );
}

pub fn set_attributes(state: &mut LogRecordState, value: &ZVal) -> phper::Result<()> {
    let iterable = crate::util::zval_iterable_to_array(value)?;
    for (key, value) in iterable.expect_z_arr()?.iter() {
        let key = match key {
            IterKey::Index(index) => index.to_string(),
            IterKey::ZStr(key) => key.to_str()?.to_string(),
        };
        set_attribute(state, key, value);
    }
    Ok(())
}

pub fn set_severity(state: &mut LogRecordState, value: &ZVal) -> phper::Result<()> {
    let number = severity_number(value)?;
    state.severity = otel_severity(number);
    if state.severity.is_none() {
        tracing::warn!("invalid OpenTelemetry log severity number {number}; value dropped");
    }
    Ok(())
}

pub fn nanos_to_system_time(nanos: i64) -> SystemTime {
    let duration = std::time::Duration::from_nanos(nanos.unsigned_abs());
    if nanos < 0 {
        std::time::UNIX_EPOCH - duration
    } else {
        std::time::UNIX_EPOCH + duration
    }
}

fn trace_context(context: &Context) -> Option<TraceContextData> {
    let span = context.span();
    let span_context = span.span_context();
    span_context.is_valid().then(|| TraceContextData {
        trace_id: span_context.trace_id(),
        span_id: span_context.span_id(),
        trace_flags: span_context.trace_flags(),
    })
}

pub fn set_context(
    state: &mut LogRecordState,
    value: &ZVal,
    context_class: &ContextClass,
) -> phper::Result<()> {
    match value.to_value()? {
        ZValRef::Null => state.context = LogContext::Current,
        ZValRef::Bool(false) => state.context = LogContext::None,
        ZValRef::Obj(object)
            if object
                .get_class()
                .is_instance_of(context_class.as_class_entry()) =>
        {
            let native = native_context_from_object(object)
                .ok_or_else(|| phper::Error::boxed("Context object has no native context state"))?;
            state.context = LogContext::Explicit(trace_context(&native));
        }
        _ => {
            return Err(phper::Error::boxed(
                "unsupported ContextInterface implementation",
            ));
        }
    }
    Ok(())
}

pub fn selected_trace_context(context: LogContext) -> Option<TraceContextData> {
    match context {
        LogContext::Current => trace_context(&Context::current()),
        LogContext::None => None,
        LogContext::Explicit(context) => context,
    }
}

fn self_return() -> ReturnType {
    ReturnType::new(ReturnTypeHint::ClassEntry("self".to_string()))
}

pub fn make_log_record_class(context_class: ContextClass) -> ClassEntity<LogRecordState> {
    let mut class =
        ClassEntity::<LogRecordState>::new_with_default_state_constructor(LOG_RECORD_CLASS_NAME);
    class.add_constant("NANOS_PER_SECOND", 1_000_000_000i64);

    class
        .add_method("__construct", Visibility::Public, |this, arguments| {
            if let Some(body) = arguments.first() {
                this.as_mut_state().body = any_value(body);
            }
            Ok::<_, phper::Error>(())
        })
        .argument(
            Argument::new("body")
                .with_type_hint(ArgumentTypeHint::Mixed)
                .with_default_value("NULL"),
        );

    class
        .add_method("setTimestamp", Visibility::Public, |this, arguments| {
            let nanos = crate::util::arg(arguments, 0)?.expect_long()?;
            this.as_mut_state().timestamp = Some(nanos_to_system_time(nanos));
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("timestamp").with_type_hint(ArgumentTypeHint::Int))
        .return_type(self_return());

    class
        .add_method(
            "setObservedTimestamp",
            Visibility::Public,
            |this, arguments| {
                this.as_mut_state().observed_timestamp = arguments
                    .first()
                    .and_then(ZVal::as_long)
                    .map(nanos_to_system_time);
                Ok::<_, phper::Error>(this.to_ref_owned())
            },
        )
        .argument(
            Argument::new("observedTimestamp")
                .with_type_hint(ArgumentTypeHint::Int)
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(self_return());

    class
        .add_method("setContext", Visibility::Public, move |this, arguments| {
            set_context(
                this.as_mut_state(),
                crate::util::arg(arguments, 0)?,
                &context_class,
            )?;
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string()))
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(self_return());

    class
        .add_method(
            "setSeverityNumber",
            Visibility::Public,
            |this, arguments| {
                set_severity(this.as_mut_state(), crate::util::arg(arguments, 0)?)?;
                Ok::<_, phper::Error>(this.to_ref_owned())
            },
        )
        .argument(
            Argument::new("severityNumber").with_type_hint(ArgumentTypeHint::Union(vec![
                ArgumentTypeHint::ClassEntry(SEVERITY_ENUM.to_string()),
                ArgumentTypeHint::Int,
            ])),
        )
        .return_type(self_return());

    class
        .add_method("setSeverityText", Visibility::Public, |this, arguments| {
            this.as_mut_state().severity_text = Some(
                crate::util::arg(arguments, 0)?
                    .expect_z_str()?
                    .to_str()?
                    .to_string(),
            );
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("severityText").with_type_hint(ArgumentTypeHint::String))
        .return_type(self_return());

    class
        .add_method("setBody", Visibility::Public, |this, arguments| {
            this.as_mut_state().body = any_value(crate::util::arg(arguments, 0)?);
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(
            Argument::new("body")
                .with_type_hint(ArgumentTypeHint::Mixed)
                .with_default_value("NULL"),
        )
        .return_type(self_return());

    class
        .add_method("setAttribute", Visibility::Public, |this, arguments| {
            let key = crate::util::arg(arguments, 0)?
                .expect_z_str()?
                .to_str()?
                .to_string();
            set_attribute(this.as_mut_state(), key, crate::util::arg(arguments, 1)?);
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("name").with_type_hint(ArgumentTypeHint::String))
        .argument(Argument::new("value").with_type_hint(ArgumentTypeHint::Mixed))
        .return_type(self_return());

    class
        .add_method("setAttributes", Visibility::Public, |this, arguments| {
            set_attributes(this.as_mut_state(), crate::util::arg(arguments, 0)?)?;
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("attributes").with_type_hint(ArgumentTypeHint::Iterable))
        .return_type(self_return());

    class
        .add_method("setEventName", Visibility::Public, |this, arguments| {
            this.as_mut_state().event_name = Some(
                crate::util::arg(arguments, 0)?
                    .expect_z_str()?
                    .to_str()?
                    .to_string(),
            );
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("eventName").with_type_hint(ArgumentTypeHint::String))
        .return_type(self_return());

    class
}
