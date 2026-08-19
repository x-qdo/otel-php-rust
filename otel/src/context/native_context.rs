use opentelemetry::Context;
use phper::{objects::ZObj, values::ZVal};
use std::{collections::HashMap, ops::Deref};

/// Request-thread-only PHP values carried alongside the thread-safe Rust
/// OpenTelemetry context. Values retain their key objects so Zend object
/// handles cannot be recycled while an entry is present.
pub struct NativeContext {
    context: Context,
    values: HashMap<u32, (ZVal, ZVal)>,
}

impl NativeContext {
    pub fn new(context: Context) -> Self {
        Self {
            context,
            values: HashMap::new(),
        }
    }

    pub fn with_context(&self, context: Context) -> Self {
        Self {
            context,
            values: self.values.clone(),
        }
    }

    pub fn with_value(&self, key: &ZObj, key_value: &ZVal, value: &ZVal) -> Self {
        let mut context = self.clone();
        if value.get_type_info().is_null() {
            context.values.remove(&key.handle());
        } else {
            context
                .values
                .insert(key.handle(), (key_value.clone(), value.clone()));
        }
        context
    }

    pub fn value(&self, key: &ZObj) -> Option<ZVal> {
        self.values
            .get(&key.handle())
            .map(|(_, value)| value.clone())
    }
}

impl Clone for NativeContext {
    fn clone(&self) -> Self {
        Self {
            context: self.context.clone(),
            values: self.values.clone(),
        }
    }
}

impl Default for NativeContext {
    fn default() -> Self {
        Self::new(Context::new())
    }
}

impl Deref for NativeContext {
    type Target = Context;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
}
