use phper::{
    arrays::{IterKey, ZArr},
    values::ZVal,
    sys::sapi_module,
};
use opentelemetry::{
    Array,
    Key,
    KeyValue,
    StringValue,
    Value,
};
use std::{
    cell::RefCell,
    collections::HashSet,
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

/// Convert a ZVal to a single KeyValue pair based on its type. The key is
/// taken by value so callers that already own a `String` pay for one copy only.
pub fn zval_to_key_value(key: impl Into<Key>, value: &ZVal) -> Option<KeyValue> {
    let type_info = value.get_type_info();
    if type_info.is_string() {
        return value.as_z_str()
            .and_then(|z| z.to_str().ok())
            .map(|s| KeyValue::new(key, s.to_string()));
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
        return zval_to_vec(key, value);
    }
    None
}

/// Convert a PHP array to a vector of KeyValue pairs without duplicating it.
pub fn zval_arr_to_key_value_vec(arr: &ZArr) -> Vec<KeyValue> {
    // nNumOfElements is an upper bound (integer keys are skipped); reserving it
    // avoids regrowing the vector for the common all-string-key attribute array.
    let capacity = unsafe { (*arr.as_ptr()).nNumOfElements } as usize;
    let mut result = Vec::with_capacity(capacity);

    for (key, value) in arr.iter() {
        match key {
            IterKey::Index(_) => {}, // Skip integer keys
            IterKey::ZStr(zstr) => {
                if let Ok(key_str) = zstr.to_str() {
                    if let Some(kv) = zval_to_key_value(attribute_key(key_str), value) {
                        result.push(kv);
                    }
                }
            },
        };
    }

    result
}

/// Get the name of the SAPI module.
pub fn get_sapi_module_name() -> String {
    unsafe { CStr::from_ptr(sapi_module.name).to_string_lossy().into_owned() }
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

fn zval_to_vec(key: impl Into<Key>, value: &ZVal) -> Option<KeyValue> {
    let array = value.as_z_arr()?;

    let mut string_values = Vec::new();
    let mut int_values = Vec::new();
    let mut float_values = Vec::new();
    let mut bool_values = Vec::new();

    for (_, v) in array.iter() {
        if let Some(val) = v.as_z_str().and_then(|z| z.to_str().ok()) {
            string_values.push(val.to_string());
        } else if let Some(val) = v.as_long() {
            int_values.push(val);
        } else if let Some(val) = v.as_double() {
            float_values.push(val);
        } else if let Some(val) = v.as_bool() {
            bool_values.push(val);
        }
    }

    if !string_values.is_empty() {
        return Some(KeyValue::new(
            key,
            Value::Array(Array::from(
                string_values.into_iter().map(StringValue::from).collect::<Vec<_>>(),
            )),
        ));
    } else if !int_values.is_empty() {
        return Some(KeyValue::new(key, Value::Array(Array::from(int_values))));
    } else if !float_values.is_empty() {
        return Some(KeyValue::new(key, Value::Array(Array::from(float_values))));
    } else if !bool_values.is_empty() {
        return Some(KeyValue::new(key, Value::Array(Array::from(bool_values))));
    }

    None
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

