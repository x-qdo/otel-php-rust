use crate::context::{
    context_class::{ContextClassEntity, clear_custom_storage, native_context_from_object},
    native_context::NativeContext,
    scope::ScopeClassEntity,
    scope_interface::{DETACHED, INACTIVE, MISMATCH},
};
use opentelemetry::{
    Context, ContextGuard,
    trace::{SpanContext, TraceContextExt},
};
use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    objects::ZObj,
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::{ExecuteData, ZVal, ZValRef},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    convert::Infallible,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

const CONTEXT_STORAGE_CLASS_NAME: &str = r"OpenTelemetry\Context\Storage";
const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";
const STORAGE_SCOPE_INTERFACE: &str = r"OpenTelemetry\Context\ContextStorageScopeInterface";

pub type StorageClass = StateClass<()>;
pub type StorageClassEntity = ClassEntity<()>;
type StoredContext = Rc<NativeContext>;
type ContextStack = Vec<(u64, StoredContext)>;

thread_local! {
    static CONTEXT_STORAGE: RefCell<HashMap<u64, StoredContext>> = RefCell::new(HashMap::new());
    static DETACHED_SPAN_STORAGE: RefCell<HashMap<u64, StoredContext>> = RefCell::new(HashMap::new());
    static GUARD_STACK: RefCell<Vec<(ContextGuard, u64)>> = const { RefCell::new(Vec::new()) };
    static MAIN_STACK: RefCell<ContextStack> = const { RefCell::new(Vec::new()) };
    static FORK_STACKS: RefCell<HashMap<String, ContextStack>> = RefCell::new(HashMap::new());
    static CURRENT_EXECUTION: RefCell<Option<String>> = const { RefCell::new(None) };
    static CONTEXT_GUARD_MAP: RefCell<HashMap<usize, ContextGuard>> = RefCell::new(HashMap::new());
}

static INSTANCE_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn current_context() -> StoredContext {
    with_active_stack(|stack| stack.last().map(|(_, context)| context.clone()))
        .unwrap_or_else(|| Rc::new(NativeContext::new(Context::current())))
}

pub fn resolve_context(instance_id: Option<u64>) -> StoredContext {
    instance_id
        .and_then(|id| get_context_instance(Some(id)))
        .unwrap_or_else(|| Rc::new(NativeContext::new(Context::current())))
}

pub fn get_context_instance(instance_id: Option<u64>) -> Option<StoredContext> {
    let id = instance_id?;
    CONTEXT_STORAGE.with(|storage| {
        storage
            .borrow()
            .get(&id)
            .cloned()
            .or_else(|| DETACHED_SPAN_STORAGE.with(|detached| detached.borrow().get(&id).cloned()))
    })
}

pub fn store_context_instance(context: StoredContext) -> Option<u64> {
    let instance_id = INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    CONTEXT_STORAGE.with(|storage| storage.borrow_mut().insert(instance_id, context));
    Some(instance_id)
}

pub fn maybe_remove_context_instance(instance_id: Option<u64>) {
    let Some(id) = instance_id else {
        return;
    };
    let remove_if_unowned = |storage: &RefCell<HashMap<u64, StoredContext>>| {
        let mut contexts = storage.borrow_mut();
        if contexts
            .get(&id)
            .is_some_and(|context| Rc::strong_count(context) == 1)
        {
            contexts.remove(&id);
        }
    };
    CONTEXT_STORAGE.with(remove_if_unowned);
    DETACHED_SPAN_STORAGE.with(remove_if_unowned);
}

pub fn remove_context_instance(instance_id: u64) {
    CONTEXT_STORAGE.with(|storage| storage.borrow_mut().remove(&instance_id));
    DETACHED_SPAN_STORAGE.with(|storage| storage.borrow_mut().remove(&instance_id));
}

fn move_detached_span_context(instance_id: u64) {
    let context = CONTEXT_STORAGE.with(|storage| storage.borrow_mut().remove(&instance_id));
    let Some(context) = context else {
        return;
    };
    if context.span().span_context().is_valid() && context.span().is_recording() {
        DETACHED_SPAN_STORAGE.with(|storage| {
            storage.borrow_mut().insert(instance_id, context);
        });
    }
}

pub fn remove_detached_span_context(span_context: &SpanContext) {
    DETACHED_SPAN_STORAGE.with(|storage| {
        storage
            .borrow_mut()
            .retain(|_, context| context.span().span_context() != span_context);
    });
}

pub fn attach_context(instance_id: Option<u64>) -> Result<(), &'static str> {
    let id = instance_id.ok_or("No context id provided")?;
    let context = get_context_instance(Some(id)).ok_or("Context not found")?;
    let guard = (**context).clone().attach();
    with_active_stack(|stack| stack.push((id, context)));
    GUARD_STACK.with(|stack| stack.borrow_mut().push((guard, id)));
    Ok(())
}

pub fn detach_context(instance_id: Option<u64>, execution_id: Option<&str>) -> i64 {
    let Some(id) = instance_id else {
        return DETACHED;
    };
    if current_execution_id().as_deref() != execution_id {
        return INACTIVE;
    }
    let status = GUARD_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        match stack.last() {
            None => INACTIVE,
            Some((_, stack_id)) if *stack_id != id => MISMATCH,
            Some(_) => {
                stack.pop();
                0
            }
        }
    });
    if status == 0 {
        with_active_stack(|stack| {
            if stack.last().is_some_and(|(stack_id, _)| *stack_id == id) {
                stack.pop();
            }
        });
        move_detached_span_context(id);
    }
    status
}

pub fn current_context_instance_id() -> Option<u64> {
    with_active_stack(|stack| stack.last().map(|(id, _)| *id))
}

pub fn current_execution_id() -> Option<String> {
    CURRENT_EXECUTION.with(|current| current.borrow().clone())
}

fn with_active_stack<R>(callback: impl FnOnce(&mut ContextStack) -> R) -> R {
    CURRENT_EXECUTION.with(|current| {
        let current = current.borrow().clone();
        match current {
            Some(id) => FORK_STACKS.with(|forks| {
                let mut forks = forks.borrow_mut();
                callback(forks.entry(id).or_default())
            }),
            None => MAIN_STACK.with(|stack| callback(&mut stack.borrow_mut())),
        }
    })
}

fn execution_id(value: &ZVal) -> phper::Result<String> {
    match value.to_value()? {
        ZValRef::Long(value) => Ok(value.to_string()),
        ZValRef::Str(value) => Ok(value.to_str()?.to_string()),
        _ => Err(phper::Error::boxed("execution context id must be int|string")),
    }
}

fn fork_execution(id: String) {
    let stack = with_active_stack(|stack| stack.clone());
    FORK_STACKS.with(|forks| {
        forks.borrow_mut().insert(id, stack);
    });
}

fn switch_execution(id: &str) {
    let fork = FORK_STACKS.with(|forks| forks.borrow().get(id).cloned());
    let (execution, stack) = match fork {
        Some(stack) => (Some(id.to_string()), stack),
        None => (None, MAIN_STACK.with(|stack| stack.borrow().clone())),
    };

    GUARD_STACK.with(|guards| {
        let mut guards = guards.borrow_mut();
        while guards.pop().is_some() {}
        for (context_id, context) in &stack {
            guards.push((Context::clone(context.as_ref()).attach(), *context_id));
        }
    });
    CURRENT_EXECUTION.with(|current| *current.borrow_mut() = execution);
}

fn destroy_execution(id: &str) {
    if current_execution_id().as_deref() != Some(id) {
        FORK_STACKS.with(|forks| {
            forks.borrow_mut().remove(id);
        });
    }
}

pub fn current_span_context_instance_id() -> Option<u64> {
    if let Some(id) = current_context_instance_id() {
        return Some(id);
    }
    let current = Context::current();
    let span_context = current.span().span_context().clone();
    if !span_context.is_valid() {
        return None;
    }
    let find = |storage: &RefCell<HashMap<u64, StoredContext>>| {
        storage.borrow().iter().find_map(|(id, context)| {
            (context.span().span_context() == &span_context).then_some(*id)
        })
    };
    CONTEXT_STORAGE
        .with(find)
        .or_else(|| DETACHED_SPAN_STORAGE.with(find))
        .or_else(|| store_context_instance(Rc::new(NativeContext::new(current))))
}

pub fn new_storage_class() -> StorageClassEntity {
    ClassEntity::new_with_default_state_constructor(CONTEXT_STORAGE_CLASS_NAME)
}

pub fn build_storage_class(
    class: &mut StorageClassEntity,
    scope_class_entity: &ScopeClassEntity,
    context_class_entity: &ContextClassEntity,
    context_storage_interface: &Interface,
    execution_context_aware_interface: &Interface,
) {
    let scope_class = scope_class_entity.bound_class();
    let context_class = context_class_entity.bound_class();
    class.set_final();
    class.implements(context_storage_interface.clone());
    class.implements(execution_context_aware_interface.clone());
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method("current", Visibility::Public, move |_, _| {
            let context = current_context();
            let mut object = context_class.init_object()?;
            *object.as_mut_state() = Some(context);
            object.set_property(
                "context_id",
                current_context_instance_id().unwrap_or(0) as i64,
            );
            Ok::<_, phper::Error>(object)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_INTERFACE.to_string(),
        )));

    let attach_scope_class = scope_class.clone();
    class
        .add_method("attach", Visibility::Public, move |_, arguments| {
            let context_object: &mut ZObj =
                crate::util::arg_mut(arguments, 0)?.expect_mut_z_obj()?;
            let mut instance_id = context_object
                .get_property("context_id")
                .as_long()
                .and_then(|id| (id > 0).then_some(id as u64));
            if get_context_instance(instance_id).is_none() {
                let context = native_context_from_object(context_object).ok_or_else(|| {
                    phper::Error::boxed("unsupported ContextInterface implementation")
                })?;
                instance_id = store_context_instance(context);
                context_object.set_property("context_id", instance_id.unwrap_or(0) as i64);
            }
            attach_context(instance_id).map_err(phper::Error::boxed)?;
            let mut object = attach_scope_class.init_object()?;
            object.as_mut_state().context = get_context_instance(instance_id);
            object.as_mut_state().execution_id = current_execution_id();
            object.set_property("context_id", instance_id.unwrap_or(0) as i64);
            Ok::<_, phper::Error>(object)
        })
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string())),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            STORAGE_SCOPE_INTERFACE.to_string(),
        )));

    class
        .add_method("scope", Visibility::Public, move |_, _| {
            let Some(context_id) = current_context_instance_id() else {
                return Ok::<_, phper::Error>(ZVal::default());
            };
            let mut object = scope_class.init_object()?;
            object.as_mut_state().context = get_context_instance(Some(context_id));
            object.as_mut_state().execution_id = current_execution_id();
            object.set_property("context_id", context_id as i64);
            Ok(ZVal::from(object))
        })
        .return_type(
            ReturnType::new(ReturnTypeHint::ClassEntry(
                STORAGE_SCOPE_INTERFACE.to_string(),
            ))
            .allow_null(),
        );

    let id_argument = || {
        Argument::new("id").with_type_hint(ArgumentTypeHint::Union(vec![
            ArgumentTypeHint::Int,
            ArgumentTypeHint::String,
        ]))
    };
    class
        .add_method("fork", Visibility::Public, |_, arguments| {
            fork_execution(execution_id(crate::util::arg(arguments, 0)?)?);
            Ok::<_, phper::Error>(())
        })
        .argument(id_argument())
        .return_type(ReturnType::new(ReturnTypeHint::Void));
    class
        .add_method("switch", Visibility::Public, |_, arguments| {
            let id = execution_id(crate::util::arg(arguments, 0)?)?;
            switch_execution(&id);
            Ok::<_, phper::Error>(())
        })
        .argument(id_argument())
        .return_type(ReturnType::new(ReturnTypeHint::Void));
    class
        .add_method("destroy", Visibility::Public, |_, arguments| {
            let id = execution_id(crate::util::arg(arguments, 0)?)?;
            destroy_execution(&id);
            Ok::<_, phper::Error>(())
        })
        .argument(id_argument())
        .return_type(ReturnType::new(ReturnTypeHint::Void));
}

pub fn get_context_ids() -> Vec<u64> {
    let mut keys: Vec<u64> =
        CONTEXT_STORAGE.with(|storage| storage.borrow().keys().copied().collect());
    DETACHED_SPAN_STORAGE.with(|storage| keys.extend(storage.borrow().keys().copied()));
    keys.sort_unstable();
    keys.dedup();
    keys
}

pub fn store_guard(exec_data: *mut ExecuteData, guard: ContextGuard) {
    let key = exec_data as usize;
    CONTEXT_GUARD_MAP.with(|map| map.borrow_mut().insert(key, guard));
}

pub fn take_guard(exec_data: *mut ExecuteData) -> Option<ContextGuard> {
    let key = exec_data as usize;
    CONTEXT_GUARD_MAP.with(|map| map.borrow_mut().remove(&key))
}

pub fn clear_context_storage() {
    CONTEXT_STORAGE.with(|storage| storage.borrow_mut().clear());
    DETACHED_SPAN_STORAGE.with(|storage| storage.borrow_mut().clear());
    GUARD_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        while stack.pop().is_some() {}
    });
    MAIN_STACK.with(|stack| stack.borrow_mut().clear());
    FORK_STACKS.with(|forks| forks.borrow_mut().clear());
    CURRENT_EXECUTION.with(|current| *current.borrow_mut() = None);
    CONTEXT_GUARD_MAP.with(|map| map.borrow_mut().clear());
    clear_custom_storage();
}
