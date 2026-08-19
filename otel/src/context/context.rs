use crate::context::{
    context_key::{CONTEXT_KEY_INTERFACE, ContextKeyClass},
    context_storage_interface::{EXECUTION_CONTEXT_AWARE_INTERFACE, STORAGE_INTERFACE},
    native_context::NativeContext,
    scope::ScopeClassEntity,
    storage::{self, StorageClassEntity},
};
use phper::{
    alloc::ToRefOwned,
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    objects::{StateObject, ZObj},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};
use std::{cell::RefCell, convert::Infallible, rc::Rc};

pub const CONTEXT_CLASS_NAME: &str = r"OpenTelemetry\Context\Context";
const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";
const SCOPE_INTERFACE: &str = r"OpenTelemetry\Context\ScopeInterface";
const IMPLICIT_INTERFACE: &str = r"OpenTelemetry\Context\ImplicitContextKeyedInterface";

pub type ContextClass = StateClass<Option<Rc<NativeContext>>>;
pub type ContextClassEntity = ClassEntity<Option<Rc<NativeContext>>>;

thread_local! {
    static CUSTOM_STORAGE: RefCell<Option<ZVal>> = const { RefCell::new(None) };
}

fn missing_state() -> phper::Error {
    phper::Error::boxed("Context object has no native context state")
}

fn custom_storage() -> Option<ZVal> {
    CUSTOM_STORAGE.with(|storage| storage.borrow().clone())
}

pub fn clear_custom_storage() {
    CUSTOM_STORAGE.with(|storage| *storage.borrow_mut() = None);
}

pub fn current_context_value(context_class: &ContextClass) -> phper::Result<ZVal> {
    if let Some(mut custom) = custom_storage() {
        return custom.expect_mut_z_obj()?.call("current", []);
    }
    let context = storage::current_context();
    Ok(ZVal::from(init_context_object(
        context_class,
        context,
        storage::current_context_instance_id(),
    )?))
}

pub fn new_context_class() -> ContextClassEntity {
    ClassEntity::new_with_default_state_constructor(CONTEXT_CLASS_NAME)
}

pub fn native_context_from_object(object: &ZObj) -> Option<Rc<NativeContext>> {
    let is_native = object
        .get_class()
        .get_name()
        .to_str()
        .is_ok_and(|name| name == CONTEXT_CLASS_NAME);
    if !is_native {
        return None;
    }
    let state = unsafe { object.as_state_obj::<Option<Rc<NativeContext>>>() };
    state
        .as_state()
        .clone()
        .or_else(|| storage::get_context_instance(get_instance_id(object)))
}

pub fn init_context_object(
    class: &ContextClass,
    context: Rc<NativeContext>,
    instance_id: Option<u64>,
) -> phper::Result<StateObject<Option<Rc<NativeContext>>>> {
    let mut object = class.init_object()?;
    *object.as_mut_state() = Some(context);
    object.set_property("context_id", instance_id.unwrap_or(0) as i64);
    Ok(object)
}

pub fn build_context_class(
    class: &mut ContextClassEntity,
    scope_class: &ScopeClassEntity,
    storage_class: &StorageClassEntity,
    key_class: ContextKeyClass,
    context_interface: Interface,
) {
    let context_class = class.bound_class();
    let scope_class = scope_class.bound_class();
    let storage_class = storage_class.bound_class();

    class.set_final();
    class.state_cloner(Clone::clone);
    class.implements(context_interface);
    class.add_property("context_id", Visibility::Private, 0i64);
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_static_method("createKey", Visibility::Public, move |arguments| {
            let name = crate::util::arg(arguments, 0)?
                .expect_z_str()?
                .to_str()?
                .to_string();
            let mut object = key_class.init_object()?;
            *object.as_mut_state() = Some(name);
            Ok::<_, phper::Error>(object)
        })
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_KEY_INTERFACE.to_string(),
        )));

    let root_context_class = context_class.clone();
    class
        .add_static_method("getRoot", Visibility::Public, move |_| {
            init_context_object(
                &root_context_class,
                Rc::new(NativeContext::default()),
                None,
            )
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_INTERFACE.to_string(),
        )));

    let current_context_class = context_class.clone();
    class
        .add_static_method("getCurrent", Visibility::Public, move |_| {
            current_context_value(&current_context_class)
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_INTERFACE.to_string(),
        )));

    let resolve_context_class = context_class.clone();
    class
        .add_static_method("resolve", Visibility::Public, move |arguments| {
            let context = crate::util::arg(arguments, 0)?;
            if context.as_z_obj().is_some() {
                return Ok::<_, phper::Error>(context.clone());
            }

            if context.get_type_info().is_false() {
                return Ok(ZVal::from(init_context_object(
                    &resolve_context_class,
                    Rc::new(NativeContext::default()),
                    None,
                )?));
            }

            if let Some(mut context_storage) = arguments
                .get(1)
                .filter(|value| value.as_z_obj().is_some())
                .cloned()
            {
                let current = context_storage.expect_mut_z_obj()?.call("current", [])?;
                if current.as_z_obj().is_some() {
                    return Ok(current);
                }
            } else if let Some(mut custom) = custom_storage() {
                let current = custom.expect_mut_z_obj()?.call("current", [])?;
                if current.as_z_obj().is_some() {
                    return Ok(current);
                }
            } else {
                return Ok(ZVal::from(init_context_object(
                    &resolve_context_class,
                    storage::current_context(),
                    storage::current_context_instance_id(),
                )?));
            }

            Ok(ZVal::from(init_context_object(
                &resolve_context_class,
                Rc::new(NativeContext::default()),
                None,
            )?))
        })
        .argument(
            Argument::new("context").with_type_hint(ArgumentTypeHint::Union(vec![
                ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string()),
                ArgumentTypeHint::False,
                ArgumentTypeHint::Null,
            ])),
        )
        .argument(
            Argument::new("contextStorage")
                .with_type_hint(ArgumentTypeHint::ClassEntry(STORAGE_INTERFACE.to_string()))
                .allow_null()
                .with_default_value("NULL"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_INTERFACE.to_string(),
        )));

    class
        .add_static_method("setStorage", Visibility::Public, |arguments| {
            let storage = crate::util::arg(arguments, 0)?.clone();
            CUSTOM_STORAGE.with(|slot| *slot.borrow_mut() = Some(storage));
            Ok::<_, phper::Error>(())
        })
        .argument(
            Argument::new("storage").with_type_hint(ArgumentTypeHint::Intersection(vec![
                STORAGE_INTERFACE.to_string(),
                EXECUTION_CONTEXT_AWARE_INTERFACE.to_string(),
            ])),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    class
        .add_static_method("storage", Visibility::Public, move |_| {
            if let Some(storage) = custom_storage() {
                return Ok::<_, phper::Error>(storage);
            }
            Ok(ZVal::from(storage_class.init_object()?))
        })
        .return_type(ReturnType::new(ReturnTypeHint::Intersection(vec![
            STORAGE_INTERFACE.to_string(),
            EXECUTION_CONTEXT_AWARE_INTERFACE.to_string(),
        ])));

    let with_context_class = context_class.clone();
    class
        .add_method("with", Visibility::Public, move |this, arguments| {
            let context = this
                .as_state()
                .clone()
                .or_else(|| storage::get_context_instance(get_instance_id(this)))
                .ok_or_else(missing_state)?;
            let key_value = crate::util::arg(arguments, 0)?;
            let key = key_value.expect_z_obj()?;
            let value = crate::util::arg(arguments, 1)?;
            let context = Rc::new(context.with_value(key, key_value, value));
            init_context_object(&with_context_class, context, None)
        })
        .argument(
            Argument::new("key").with_type_hint(ArgumentTypeHint::ClassEntry(
                CONTEXT_KEY_INTERFACE.to_string(),
            )),
        )
        .argument(Argument::new("value"))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            "self".to_string(),
        )));

    class
        .add_method("get", Visibility::Public, |this, arguments| {
            let context = this
                .as_state()
                .clone()
                .or_else(|| storage::get_context_instance(get_instance_id(this)))
                .ok_or_else(missing_state)?;
            let key = crate::util::arg(arguments, 0)?.expect_z_obj()?;
            Ok::<_, phper::Error>(context.value(key).unwrap_or_default())
        })
        .argument(
            Argument::new("key").with_type_hint(ArgumentTypeHint::ClassEntry(
                CONTEXT_KEY_INTERFACE.to_string(),
            )),
        );

    class
        .add_method("withContextValue", Visibility::Public, |this, arguments| {
            let mut value = crate::util::arg(arguments, 0)?.clone();
            let context = ZVal::from(this.to_ref_owned());
            value
                .expect_mut_z_obj()?
                .call("storeInContext", &mut [context])
        })
        .argument(
            Argument::new("value")
                .with_type_hint(ArgumentTypeHint::ClassEntry(IMPLICIT_INTERFACE.to_string())),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            CONTEXT_INTERFACE.to_string(),
        )));

    class
        .add_method("activate", Visibility::Public, move |this, _| {
            if let Some(mut custom) = custom_storage() {
                return custom
                    .expect_mut_z_obj()?
                    .call("attach", &mut [ZVal::from(this.to_ref_owned())]);
            }
            let context = this.as_state().as_ref().ok_or_else(missing_state)?.clone();
            let instance_id = storage::store_context_instance(context.clone());
            this.set_property("context_id", instance_id.unwrap_or(0) as i64);
            storage::attach_context(instance_id).map_err(phper::Error::boxed)?;
            let mut object = scope_class.init_object()?;
            object.as_mut_state().context = Some(context);
            object.as_mut_state().execution_id = storage::current_execution_id();
            object.set_property("context_id", instance_id.unwrap_or(0) as i64);
            Ok::<_, phper::Error>(ZVal::from(object))
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SCOPE_INTERFACE.to_string(),
        )));
}

pub fn get_instance_id(object: &ZObj) -> Option<u64> {
    object
        .get_property("context_id")
        .as_long()
        .and_then(|id| (id > 0).then_some(id as u64))
}
