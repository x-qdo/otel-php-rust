use opentelemetry::{Array, Key, KeyValue, StringValue, Value};
use opentelemetry_sdk::trace::SpanLimits;
use phper::{
    arrays::{IterKey, ZArr},
    errors::ArgumentCountError,
    functions,
    sys::sapi_module,
    values::{ExecuteData, ZVal},
};
use std::{
    cell::RefCell,
    collections::HashSet,
    env,
    ffi::CStr,
    sync::{LazyLock, Mutex},
};

static INTERNED_STRINGS: LazyLock<Mutex<HashSet<&'static str>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Intern a string for the process lifetime. Used for instrumentation scope
/// names, versions and schema URLs so `SdkTracer`/`InstrumentationScope`
/// clones on the span hot path borrow a `'static` str instead of allocating.
/// The set of distinct scope identifiers is bounded by the application code
/// calling `getTracer()`, mirroring the official SDK's per-scope tracer cache.
pub fn intern(value: &str) -> &'static str {
    let mut interned = INTERNED_STRINGS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = interned.get(value) {
        return existing;
    }
    let leaked: &'static str = Box::leak(value.to_owned().into_boxed_str());
    interned.insert(leaked);
    leaked
}

/// Upper bound on distinct attribute keys interned per thread. Keys are a small
/// fixed vocabulary in practice; anything beyond the bound falls back to an owned
/// key so a pathological caller cannot grow the table without limit.
const MAX_INTERNED_ATTRIBUTE_KEYS: usize = 4_096;

thread_local! {
    static ATTRIBUTE_KEYS: RefCell<HashSet<&'static str>> = RefCell::new(HashSet::new());
    static ATTRIBUTE_LIMITS: RefCell<Option<AttributeLimitConfig>> = const { RefCell::new(None) };
}

const DEFAULT_ATTRIBUTE_COUNT_LIMIT: usize = 128;
const DEFAULT_ATTRIBUTE_KEY_LENGTH_LIMIT: usize = 256;
const DEFAULT_ATTRIBUTE_ARRAY_LENGTH_LIMIT: usize = 128;
const MAX_CONFIGURED_LIMIT: usize = 1_000_000;

static INVALID_LIMIT_WARNINGS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(Clone, Copy, Debug)]
pub enum AttributeDestination {
    Span,
    Event,
    Link,
    Log,
    Metric,
    Scope,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AttributeLimits {
    pub count: usize,
    pub key_length: usize,
    pub value_length: usize,
    pub array_length: usize,
}

#[derive(Clone, Copy, Debug)]
struct AttributeLimitConfig {
    span: AttributeLimits,
    event: AttributeLimits,
    link: AttributeLimits,
    log: AttributeLimits,
    metric: AttributeLimits,
    scope: AttributeLimits,
    max_events_per_span: usize,
    max_links_per_span: usize,
}

impl AttributeLimitConfig {
    fn from_env() -> Self {
        let general_count =
            configured_limit("OTEL_ATTRIBUTE_COUNT_LIMIT", DEFAULT_ATTRIBUTE_COUNT_LIMIT);
        let general_value = configured_limit(
            "OTEL_ATTRIBUTE_VALUE_LENGTH_LIMIT",
            usize::MAX,
        );
        let key_length = configured_limit(
            "OTEL_PHP_ATTRIBUTE_KEY_LENGTH_LIMIT",
            DEFAULT_ATTRIBUTE_KEY_LENGTH_LIMIT,
        );
        let array_length = configured_limit(
            "OTEL_PHP_ATTRIBUTE_ARRAY_LENGTH_LIMIT",
            DEFAULT_ATTRIBUTE_ARRAY_LENGTH_LIMIT,
        );

        let span_value = configured_limit("OTEL_SPAN_ATTRIBUTE_VALUE_LENGTH_LIMIT", general_value);
        let limits = |count_name: &str, value_length: usize| AttributeLimits {
            count: configured_limit(count_name, general_count),
            key_length,
            value_length,
            array_length,
        };
        let general = AttributeLimits {
            count: general_count,
            key_length,
            value_length: general_value,
            array_length,
        };

        Self {
            span: limits("OTEL_SPAN_ATTRIBUTE_COUNT_LIMIT", span_value),
            event: limits("OTEL_EVENT_ATTRIBUTE_COUNT_LIMIT", span_value),
            link: limits("OTEL_LINK_ATTRIBUTE_COUNT_LIMIT", span_value),
            log: limits(
                "OTEL_LOGRECORD_ATTRIBUTE_COUNT_LIMIT",
                configured_limit("OTEL_LOGRECORD_ATTRIBUTE_VALUE_LENGTH_LIMIT", general_value),
            ),
            // Metric attributes identify time series and are explicitly exempt
            // from common attribute truncation/deletion rules in the OTel spec.
            metric: AttributeLimits {
                count: usize::MAX,
                key_length: usize::MAX,
                value_length: usize::MAX,
                array_length: usize::MAX,
            },
            scope: general,
            max_events_per_span: configured_limit("OTEL_SPAN_EVENT_COUNT_LIMIT", 128),
            max_links_per_span: configured_limit("OTEL_SPAN_LINK_COUNT_LIMIT", 128),
        }
    }

    fn for_destination(self, destination: AttributeDestination) -> AttributeLimits {
        match destination {
            AttributeDestination::Span => self.span,
            AttributeDestination::Event => self.event,
            AttributeDestination::Link => self.link,
            AttributeDestination::Log => self.log,
            AttributeDestination::Metric => self.metric,
            AttributeDestination::Scope => self.scope,
        }
    }
}

fn warn_invalid_limit_once(name: &str, value: &str) {
    let key = format!("{name}={value}");
    let mut warnings = INVALID_LIMIT_WARNINGS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if warnings.insert(key) {
        tracing::warn!(
            "Invalid {name} value {value:?}; expected an integer from 0 to {MAX_CONFIGURED_LIMIT}, using the fallback"
        );
    }
}

fn configured_limit(name: &str, fallback: usize) -> usize {
    match env::var(name) {
        Ok(value) if value.trim().is_empty() => fallback,
        Ok(value) => match value.trim().parse::<usize>() {
            Ok(value) if value <= MAX_CONFIGURED_LIMIT => value,
            _ => {
                warn_invalid_limit_once(name, &value);
                fallback
            }
        },
        Err(env::VarError::NotPresent) => fallback,
        Err(env::VarError::NotUnicode(_)) => {
            warn_invalid_limit_once(name, "<non-unicode>");
            fallback
        }
    }
}

fn attribute_limit_config() -> AttributeLimitConfig {
    ATTRIBUTE_LIMITS.with(|cache| {
        if let Some(config) = *cache.borrow() {
            return config;
        }
        let config = AttributeLimitConfig::from_env();
        *cache.borrow_mut() = Some(config);
        config
    })
}

pub(crate) fn attribute_limits(destination: AttributeDestination) -> AttributeLimits {
    attribute_limit_config().for_destination(destination)
}

/// Resolve attribute limits after request-local environment configuration has
/// been imported. Public APIs also initialize lazily when automatic request
/// instrumentation is disabled.
pub fn begin_request_attribute_limits() {
    ATTRIBUTE_LIMITS.with(|cache| *cache.borrow_mut() = Some(AttributeLimitConfig::from_env()));
}

/// Drop request-local limit configuration before the next request mutates the
/// process environment.
pub fn end_request_attribute_limits() {
    ATTRIBUTE_LIMITS.with(|cache| *cache.borrow_mut() = None);
}

/// SDK collection limits corresponding to the same configuration used while
/// converting PHP values.
pub fn trace_span_limits() -> SpanLimits {
    let config = attribute_limit_config();
    SpanLimits {
        max_events_per_span: config.max_events_per_span as u32,
        max_attributes_per_span: config.span.count as u32,
        max_links_per_span: config.max_links_per_span as u32,
        max_attributes_per_event: config.event.count as u32,
        max_attributes_per_link: config.link.count as u32,
    }
}

/// Build an attribute [`Key`] without allocating when the key has been seen
/// before on this thread. Interned keys are `'static`, so every later
/// `KeyValue` clone (builder → span → export data) copies a pointer instead
/// of a heap string.
pub fn attribute_key(name: &str) -> Key {
    ATTRIBUTE_KEYS.with(|keys| {
        intern_attribute_key(&mut keys.borrow_mut(), name, MAX_INTERNED_ATTRIBUTE_KEYS)
    })
}

fn intern_attribute_key(table: &mut HashSet<&'static str>, name: &str, capacity: usize) -> Key {
    if let Some(existing) = table.get(name) {
        return Key::from_static_str(existing);
    }
    if table.len() >= capacity {
        return Key::new(name.to_string());
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    table.insert(leaked);
    Key::from_static_str(leaked)
}

/// Checked positional argument access for PHP handlers. phper only guarantees
/// `arguments.len() >= declared required count`; methods that declare fewer
/// (or no) parameters than they read must report a missing argument as PHP's
/// `ArgumentCountError` instead of panicking on a slice index.
pub fn arg(arguments: &[ZVal], index: usize) -> phper::Result<&ZVal> {
    arguments
        .get(index)
        .ok_or_else(|| missing_argument(index, arguments.len()))
}

/// Mutable variant of [`arg`].
pub fn arg_mut(arguments: &mut [ZVal], index: usize) -> phper::Result<&mut ZVal> {
    let given = arguments.len();
    arguments
        .get_mut(index)
        .ok_or_else(|| missing_argument(index, given))
}

fn missing_argument(index: usize, given: usize) -> phper::Error {
    ArgumentCountError::new(current_function_name(), index + 1, given).into()
}

/// Name of the PHP function or method whose handler is running
/// (`Class::method` or `function`), for diagnostics.
pub fn current_function_name() -> String {
    unsafe { ExecuteData::try_from_ptr(phper::eg!(current_execute_data)) }
        .and_then(|execute_data| {
            execute_data
                .func()
                .get_function_or_method_name()
                .to_str()
                .ok()
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "<unknown>".to_string())
}

/// Convert a ZVal to a single KeyValue pair based on its type. The key is
/// taken by value so callers that already own a `String` pay for one copy only.
pub fn zval_to_key_value(
    destination: AttributeDestination,
    key: impl Into<Key>,
    value: &ZVal,
) -> Option<KeyValue> {
    let limits = attribute_limits(destination);
    let key = key.into();
    if !valid_attribute_key(key.as_str(), limits) {
        return None;
    }
    let type_info = value.get_type_info();
    if type_info.is_string() {
        return value
            .as_z_str()
            .and_then(|z| z.to_str().ok())
            .map(|s| KeyValue::new(key, truncate_string(s, limits.value_length)));
    }
    if type_info.is_long() {
        return value.as_long().map(|v| KeyValue::new(key, v));
    }
    if type_info.is_double() {
        return value.as_double().map(|v| KeyValue::new(key, v));
    }
    if type_info.is_bool() {
        return value.as_bool().map(|v| KeyValue::new(key, v));
    }
    if type_info.is_array() {
        return zval_to_vec(key, value, limits);
    }
    None
}

/// Convert a PHP array to a vector of KeyValue pairs without duplicating it.
pub fn zval_arr_to_key_value_vec(arr: &ZArr, destination: AttributeDestination) -> Vec<KeyValue> {
    let limits = attribute_limits(destination);
    if limits.count == 0 {
        return Vec::new();
    }
    // nNumOfElements is an upper bound (integer keys are skipped); reserving it
    // avoids regrowing the vector for the common all-string-key attribute array.
    let capacity = unsafe { (*arr.as_ptr()).nNumOfElements } as usize;
    let mut result = Vec::with_capacity(capacity.min(limits.count));

    for (key, value) in arr.iter() {
        match key {
            IterKey::Index(_) => {} // Skip integer keys
            IterKey::ZStr(zstr) => {
                let Ok(key_str) = zstr.to_str() else {
                    continue;
                };
                let Some(kv) = zval_to_key_value(destination, attribute_key(key_str), value) else {
                    continue;
                };
                result.push(kv);
                if result.len() == limits.count {
                    break;
                }
            }
        };
    }

    result
}

/// Convert any PHP `iterable` to OpenTelemetry attributes. Arrays stay on the
/// zero-copy path; Traversable values are materialized with PHP's own iterator
/// machinery so generators and userland iterators obey the same contract.
pub fn zval_iterable_to_key_value_vec(
    value: &ZVal,
    destination: AttributeDestination,
) -> phper::Result<Vec<KeyValue>> {
    let value = zval_iterable_to_array(value)?;
    Ok(zval_arr_to_key_value_vec(
        value.expect_z_arr()?,
        destination,
    ))
}

/// Materialize a PHP iterable while keeping arrays on Zend's copy-on-write
/// path. Callers can then apply signal-specific value conversion rules.
pub fn zval_iterable_to_array(value: &ZVal) -> phper::Result<ZVal> {
    if value.as_z_arr().is_some() {
        return Ok(value.clone());
    }
    functions::call("iterator_to_array", &mut [value.clone(), ZVal::from(true)])
}

/// Get the name of the SAPI module.
pub fn get_sapi_module_name() -> String {
    unsafe {
        CStr::from_ptr(sapi_module.name)
            .to_string_lossy()
            .into_owned()
    }
}

/// Get the PHP version as a string.
pub fn get_php_version() -> String {
    let php_version = format!(
        "{}.{}.{}",
        phper::sys::PHP_MAJOR_VERSION,
        phper::sys::PHP_MINOR_VERSION,
        phper::sys::PHP_RELEASE_VERSION
    );
    php_version
}

pub(crate) fn valid_attribute_key(key: &str, limits: AttributeLimits) -> bool {
    !key.is_empty() && key.chars().count() <= limits.key_length
}

pub(crate) fn truncate_string(value: &str, max_chars: usize) -> String {
    if max_chars == usize::MAX {
        return value.to_string();
    }
    if value.chars().nth(max_chars).is_none() {
        value.to_string()
    } else {
        value.chars().take(max_chars).collect()
    }
}

fn zval_to_vec(key: impl Into<Key>, value: &ZVal, limits: AttributeLimits) -> Option<KeyValue> {
    let array = value.as_z_arr()?;
    enum ArrayKind {
        Empty,
        String(Vec<StringValue>),
        Integer(Vec<i64>),
        Numeric(Vec<f64>),
        Boolean(Vec<bool>),
    }
    let mut kind = ArrayKind::Empty;
    let mut seen = 0usize;
    for (_, v) in array.iter() {
        seen += 1;
        if let Some(val) = v.as_z_str().and_then(|z| z.to_str().ok()) {
            match &mut kind {
                ArrayKind::Empty => {
                    let mut values = Vec::with_capacity(limits.array_length.min(8));
                    if seen <= limits.array_length {
                        values.push(truncate_string(val, limits.value_length).into());
                    }
                    kind = ArrayKind::String(values);
                }
                ArrayKind::String(values) if seen <= limits.array_length => {
                    values.push(truncate_string(val, limits.value_length).into());
                }
                ArrayKind::String(_) => {}
                _ => return None,
            }
        } else if let Some(val) = v.as_long() {
            match &mut kind {
                ArrayKind::Empty => {
                    let mut values = Vec::with_capacity(limits.array_length.min(8));
                    if seen <= limits.array_length {
                        values.push(val);
                    }
                    kind = ArrayKind::Integer(values);
                }
                ArrayKind::Integer(values) if seen <= limits.array_length => values.push(val),
                ArrayKind::Integer(_) => {}
                ArrayKind::Numeric(values) if seen <= limits.array_length => {
                    values.push(val as f64)
                }
                ArrayKind::Numeric(_) => {}
                _ => return None,
            }
        } else if let Some(val) = v.as_double() {
            match &mut kind {
                ArrayKind::Empty => {
                    let mut values = Vec::with_capacity(limits.array_length.min(8));
                    if seen <= limits.array_length {
                        values.push(val);
                    }
                    kind = ArrayKind::Numeric(values);
                }
                ArrayKind::Integer(values) => {
                    let mut numeric = values.iter().map(|value| *value as f64).collect::<Vec<_>>();
                    if seen <= limits.array_length {
                        numeric.push(val);
                    }
                    kind = ArrayKind::Numeric(numeric);
                }
                ArrayKind::Numeric(values) if seen <= limits.array_length => values.push(val),
                ArrayKind::Numeric(_) => {}
                _ => return None,
            }
        } else {
            // Attribute arrays must be homogeneous primitives. A non-boolean
            // value here is invalid and drops the whole attribute.
            let val = v.as_bool()?;
            match &mut kind {
                ArrayKind::Empty => {
                    let mut values = Vec::with_capacity(limits.array_length.min(8));
                    if seen <= limits.array_length {
                        values.push(val);
                    }
                    kind = ArrayKind::Boolean(values);
                }
                ArrayKind::Boolean(values) if seen <= limits.array_length => values.push(val),
                ArrayKind::Boolean(_) => {}
                _ => return None,
            }
        }
    }
    let value = match kind {
        ArrayKind::Empty => Value::Array(Array::String(Vec::new())),
        ArrayKind::String(values) => Value::Array(Array::String(values)),
        ArrayKind::Integer(values) => Value::Array(Array::I64(values)),
        ArrayKind::Numeric(values) => Value::Array(Array::F64(values)),
        ArrayKind::Boolean(values) => Value::Array(Array::Bool(values)),
    };
    Some(KeyValue::new(key, value))
}

/// Apply configured key, value, array and count limits to attributes created
/// outside PHP zval conversion (for example exception attributes).
pub fn limit_key_values(
    attributes: impl IntoIterator<Item = KeyValue>,
    destination: AttributeDestination,
) -> Vec<KeyValue> {
    let limits = attribute_limits(destination);
    attributes
        .into_iter()
        .filter_map(|attribute| {
            valid_attribute_key(attribute.key.as_str(), limits).then(|| {
                KeyValue::new(attribute.key, limit_value(attribute.value, limits))
            })
        })
        .take(limits.count)
        .collect()
}

fn limit_value(value: Value, limits: AttributeLimits) -> Value {
    match value {
        Value::String(value) => {
            Value::String(truncate_string(value.as_str(), limits.value_length).into())
        }
        Value::Array(Array::String(values)) => Value::Array(Array::String(
            values
                .into_iter()
                .take(limits.array_length)
                .map(|value| truncate_string(value.as_str(), limits.value_length).into())
                .collect(),
        )),
        Value::Array(Array::Bool(values)) => Value::Array(Array::Bool(
            values.into_iter().take(limits.array_length).collect(),
        )),
        Value::Array(Array::I64(values)) => Value::Array(Array::I64(
            values.into_iter().take(limits.array_length).collect(),
        )),
        Value::Array(Array::F64(values)) => Value::Array(Array::F64(
            values.into_iter().take(limits.array_length).collect(),
        )),
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_static(key: &Key) -> bool {
        // A `'static` key clones without allocating; owned keys copy the string.
        let clone = key.clone();
        clone.as_str().as_ptr() == key.as_str().as_ptr()
    }

    #[test]
    fn attribute_keys_are_interned_once_and_shared_by_clones() {
        let mut table = HashSet::new();
        let first = intern_attribute_key(&mut table, "http.method", 8);
        let second = intern_attribute_key(&mut table, "http.method", 8);
        assert_eq!(first.as_str().as_ptr(), second.as_str().as_ptr());
        assert!(is_static(&first));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn attribute_key_table_stops_growing_at_capacity() {
        let mut table = HashSet::new();
        for i in 0..3 {
            intern_attribute_key(&mut table, &format!("key.{i}"), 3);
        }
        let overflow = intern_attribute_key(&mut table, "key.overflow", 3);
        assert_eq!(overflow.as_str(), "key.overflow");
        assert!(!is_static(&overflow));
        assert_eq!(table.len(), 3);
        // Known keys keep resolving after the table is full.
        assert!(is_static(&intern_attribute_key(&mut table, "key.1", 3)));
    }
}
