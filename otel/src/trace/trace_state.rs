use crate::trace::span_context_interface::TRACE_STATE_INTERFACE;
use opentelemetry::trace::TraceState as OtelTraceState;
use phper::{
    alloc::ToRefOwned,
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};
use std::{convert::Infallible, str::FromStr};

pub const TRACE_STATE_CLASS: &str = r"OpenTelemetry\API\Trace\TraceState";
pub type TraceStateClass = StateClass<TraceStateState>;

#[derive(Clone, Default)]
pub struct TraceStateState {
    members: Vec<(String, String)>,
}

impl TraceStateState {
    pub(crate) fn parse(raw: &str) -> Self {
        let mut members = Vec::new();
        for member in raw.split(',') {
            let member = member.trim_matches([' ', '\t']);
            if member.is_empty() {
                continue;
            }
            let Some((key, value)) = member.split_once('=') else {
                return Self::default();
            };
            if !Self::validate_member(&members, key, value) {
                return Self::default();
            }
            if !members.iter().any(|(existing, _)| existing == key) {
                members.push((key.to_string(), value.to_string()));
            }
        }
        Self { members }
    }

    fn valid_key(key: &str) -> bool {
        let bytes = key.as_bytes();
        if bytes.is_empty() || bytes.len() > 256 {
            return false;
        }
        let allowed = |byte: u8| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'_' | b'-' | b'*' | b'/')
        };
        let Some(first) = bytes.first() else {
            return false;
        };
        if let Some(at) = bytes.iter().position(|byte| *byte == b'@') {
            let vendor_start = at.saturating_add(1);
            let Some(vendor_first) = bytes.get(vendor_start) else {
                return false;
            };
            if bytes.iter().filter(|byte| **byte == b'@').count() != 1
                || at == 0
                || at > 241
                || bytes.len().saturating_sub(vendor_start) == 0
                || bytes.len().saturating_sub(vendor_start) > 14
                || !(first.is_ascii_lowercase() || first.is_ascii_digit())
                || !vendor_first.is_ascii_lowercase()
            {
                return false;
            }
            let (Some(tenant), Some(vendor)) =
                (bytes.get(..at), bytes.get(vendor_start..))
            else {
                return false;
            };
            tenant.iter().all(|byte| allowed(*byte))
                && vendor.iter().all(|byte| allowed(*byte))
        } else {
            first.is_ascii_lowercase() && bytes.iter().all(|byte| allowed(*byte))
        }
    }

    fn valid_value(value: &str) -> bool {
        let bytes = value.as_bytes();
        !bytes.is_empty()
            && bytes.len() <= 256
            && bytes.iter().all(|byte| (0x20..=0x7e).contains(byte))
            && (0x21..=0x7e).contains(bytes.last().unwrap_or(&0))
            && !bytes.contains(&b',')
            && !bytes.contains(&b'=')
    }

    fn validate_member(members: &[(String, String)], key: &str, value: &str) -> bool {
        let existing = members.iter().any(|(member, _)| member == key);
        (existing || Self::valid_key(key))
            && Self::valid_value(value)
            && (members.len() < 32 || existing)
    }

    pub fn header(&self) -> String {
        self.members
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn limited_header(&self, limit: Option<i64>) -> String {
        let Some(limit) = limit else {
            return self.header();
        };
        let mut members = self.members.clone();
        let mut length = members
            .iter()
            .map(|(key, value)| key.len() + 1 + value.len())
            .sum::<usize>()
            + members.len().saturating_sub(1);
        if i64::try_from(length).unwrap_or(i64::MAX) > limit {
            for threshold in [128usize, 0usize] {
                let mut index = members.len();
                while index > 0 {
                    index -= 1;
                    let Some((key, value)) = members.get(index) else {
                        continue;
                    };
                    let entry = key.len() + 1 + value.len();
                    if entry <= threshold {
                        continue;
                    }
                    members.remove(index);
                    length = length.saturating_sub(entry + usize::from(!members.is_empty()));
                    if i64::try_from(length).unwrap_or(i64::MAX) <= limit {
                        break;
                    }
                }
                if i64::try_from(length).unwrap_or(i64::MAX) <= limit {
                    break;
                }
            }
        }
        Self { members }.header()
    }
}

pub fn otel_trace_state_from_header(header: &str) -> OtelTraceState {
    OtelTraceState::from_str(header).unwrap_or_default()
}

pub fn make_trace_state_interface() -> phper::classes::InterfaceEntity {
    let mut interface = phper::classes::InterfaceEntity::new(TRACE_STATE_INTERFACE);
    let return_type = || {
        ReturnType::new(ReturnTypeHint::ClassEntry(TRACE_STATE_INTERFACE.to_string()))
    };
    interface
        .add_method("with")
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .argument(Argument::new("value").with_type_hint(ArgumentTypeHint::String))
        .return_type(return_type());
    interface
        .add_method("without")
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(return_type());
    interface
        .add_method("get")
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::String).allow_null());
    interface
        .add_method("getListMemberCount")
        .return_type(ReturnType::new(ReturnTypeHint::Int));
    interface
        .add_method("toString")
        .argument(
            Argument::new("limit")
                .with_type_hint(ArgumentTypeHint::Int)
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::String));
    interface
        .add_method("__toString")
        .return_type(ReturnType::new(ReturnTypeHint::String));
    interface
}

pub fn make_trace_state_class(interface: Interface) -> ClassEntity<TraceStateState> {
    let mut class = ClassEntity::new_with_default_state_constructor(TRACE_STATE_CLASS);
    let trace_state_class = class.bound_class();
    class.state_cloner(Clone::clone);
    class.implements(interface);
    class.add_constant("MAX_LIST_MEMBERS", 32i64);
    class.add_constant("MAX_COMBINED_LENGTH", 512i64);
    class.add_constant("LIST_MEMBERS_SEPARATOR", ",");
    class.add_constant("LIST_MEMBER_KEY_VALUE_SPLITTER", "=");

    class
        .add_method("__construct", Visibility::Public, |this, arguments| {
            let raw = arguments
                .first()
                .and_then(|value| value.as_z_str())
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            *this.as_mut_state() = TraceStateState::parse(raw);
            Ok::<_, Infallible>(())
        })
        .argument(
            Argument::new("rawTracestate")
                .with_type_hint(ArgumentTypeHint::String)
                .allow_null()
                .with_default_value("NULL"),
        );

    let with_class = trace_state_class.clone();
    class
        .add_method("with", Visibility::Public, move |this, arguments| {
            let key = crate::util::arg(arguments, 0)?.expect_z_str()?.to_str()?;
            let value = crate::util::arg(arguments, 1)?.expect_z_str()?.to_str()?;
            if !TraceStateState::validate_member(&this.as_state().members, key, value) {
                tracing::warn!("Invalid tracestate key/value for: {}", key);
                return Ok::<_, phper::Error>(phper::values::ZVal::from(this.to_ref_owned()));
            }
            let mut state = this.as_state().clone();
            state.members.retain(|(existing, _)| existing != key);
            state.members.insert(0, (key.to_string(), value.to_string()));
            let mut object = with_class.init_object()?;
            *object.as_mut_state() = state;
            Ok(phper::values::ZVal::from(object))
        })
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .argument(Argument::new("value").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            TRACE_STATE_INTERFACE.to_string(),
        )));

    let without_class = trace_state_class.clone();
    class
        .add_method("without", Visibility::Public, move |this, arguments| {
            let key = crate::util::arg(arguments, 0)?.expect_z_str()?.to_str()?;
            if !this.as_state().members.iter().any(|(existing, _)| existing == key) {
                return Ok::<_, phper::Error>(phper::values::ZVal::from(this.to_ref_owned()));
            }
            let mut state = this.as_state().clone();
            state.members.retain(|(existing, _)| existing != key);
            let mut object = without_class.init_object()?;
            *object.as_mut_state() = state;
            Ok(phper::values::ZVal::from(object))
        })
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            TRACE_STATE_INTERFACE.to_string(),
        )));

    class
        .add_method("get", Visibility::Public, |this, arguments| {
            let key = crate::util::arg(arguments, 0)?.expect_z_str()?.to_str()?;
            Ok::<_, phper::Error>(
                this.as_state()
                    .members
                    .iter()
                    .find(|(existing, _)| existing == key)
                    .map(|(_, value)| value.clone()),
            )
        })
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::String).allow_null());
    class
        .add_method("getListMemberCount", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.as_state().members.len() as i64)
        })
        .return_type(ReturnType::new(ReturnTypeHint::Int));
    class
        .add_method("toString", Visibility::Public, |this, arguments| {
            let limit = arguments.first().and_then(|value| value.as_long());
            Ok::<_, Infallible>(this.as_state().limited_header(limit))
        })
        .argument(
            Argument::new("limit")
                .with_type_hint(ArgumentTypeHint::Int)
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::String));
    class
        .add_method("__toString", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(this.as_state().header())
        })
        .return_type(ReturnType::new(ReturnTypeHint::String));

    for method in ["logDebug", "logInfo", "logNotice", "logWarning", "logError"] {
        class
            .add_static_method(method, Visibility::Protected, |_| Ok::<_, Infallible>(()))
            .argument(Argument::new("message").with_type_hint(ArgumentTypeHint::String))
            .argument(
                Argument::new("context")
                    .with_type_hint(ArgumentTypeHint::Array)
                    .with_default_value("[]"),
            )
            .return_type(ReturnType::new(ReturnTypeHint::Void));
    }

    class
}
