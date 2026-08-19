use crate::{
    context::{
        context_class::{ContextClassEntity, get_instance_id},
        native_context::NativeContext,
        storage,
    },
    trace::local_root_span,
};
use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::{ZVal, ZValRef},
};
use std::{collections::HashMap, convert::Infallible, rc::Rc};

const SCOPE_CLASS_NAME: &str = r"OpenTelemetry\Context\Scope";
#[derive(Default)]
pub struct ScopeState {
    pub context: Option<Rc<NativeContext>>,
    pub execution_id: Option<String>,
    detached: bool,
    offsets: HashMap<String, ZVal>,
    next_offset: usize,
}

pub type ScopeClass = StateClass<ScopeState>;
pub type ScopeClassEntity = ClassEntity<ScopeState>;

pub fn new_scope_class() -> ScopeClassEntity {
    ScopeClassEntity::new_with_default_state_constructor(SCOPE_CLASS_NAME)
}

pub fn build_scope_class(
    class: &mut ScopeClassEntity,
    context_class: &ContextClassEntity,
    scope_interface: &Interface,
) {
    let context_ce = context_class.bound_class();
    class.implements(scope_interface.clone());
    class.add_property("context_id", Visibility::Private, 0i64);

    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method(
            "detach",
            Visibility::Public,
            |this, _| -> phper::Result<i64> {
                if this.as_state().detached {
                    return Ok(crate::context::scope_interface::DETACHED);
                }
                let instance_id = get_instance_id(this);
                let status = storage::detach_context(
                    instance_id,
                    this.as_state().execution_id.as_deref(),
                );
                if status == 0 {
                    local_root_span::maybe_remove_local_root_span(instance_id);
                    this.as_mut_state().detached = true;
                }
                Ok(status)
            },
        )
        .return_type(ReturnType::new(ReturnTypeHint::Int));

    class
        .add_method("context", Visibility::Public, move |this, _| {
            let instance_id = get_instance_id(this);
            let ctx = this.as_state().context.clone();
            let mut object = context_ce.init_object()?;
            *object.as_mut_state() = ctx;
            object.set_property("context_id", instance_id.unwrap_or(0) as i64);
            Ok::<_, phper::Error>(object)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(String::from(
            r"OpenTelemetry\Context\ContextInterface",
        ))));

    class
        .add_method("offsetExists", Visibility::Public, |this, arguments| {
            let key = offset_key(crate::util::arg(arguments, 0)?);
            Ok::<_, phper::Error>(key.is_some_and(|key| this.as_state().offsets.contains_key(&key)))
        })
        .argument(Argument::new("offset").with_type_hint(ArgumentTypeHint::Mixed))
        .return_type(ReturnType::new(ReturnTypeHint::Bool));
    class
        .add_method("offsetGet", Visibility::Public, |this, arguments| {
            let value = offset_key(crate::util::arg(arguments, 0)?)
                .and_then(|key| this.as_state().offsets.get(&key).cloned())
                .unwrap_or_default();
            Ok::<_, phper::Error>(value)
        })
        .argument(Argument::new("offset").with_type_hint(ArgumentTypeHint::Mixed))
        .return_type(ReturnType::new(ReturnTypeHint::Mixed));
    class
        .add_method("offsetSet", Visibility::Public, |this, arguments| {
            let key = offset_key(crate::util::arg(arguments, 0)?).unwrap_or_else(|| {
                let key = this.as_state().next_offset.to_string();
                this.as_mut_state().next_offset += 1;
                key
            });
            let value = crate::util::arg(arguments, 1)?.clone();
            this.as_mut_state().offsets.insert(key, value);
            Ok::<_, phper::Error>(())
        })
        .argument(Argument::new("offset"))
        .argument(Argument::new("value"))
        .return_type(ReturnType::new(ReturnTypeHint::Void));
    class
        .add_method("offsetUnset", Visibility::Public, |this, arguments| {
            if let Some(key) = offset_key(crate::util::arg(arguments, 0)?) {
                this.as_mut_state().offsets.remove(&key);
            }
            Ok::<_, phper::Error>(())
        })
        .argument(Argument::new("offset").with_type_hint(ArgumentTypeHint::Mixed))
        .return_type(ReturnType::new(ReturnTypeHint::Void));
}

fn offset_key(value: &ZVal) -> Option<String> {
    match value.to_value().ok()? {
        ZValRef::Str(value) => value.to_str().ok().map(str::to_string),
        ZValRef::Long(value) => Some(value.to_string()),
        ZValRef::Null => None,
        _ => Some(format!("{value:?}")),
    }
}
