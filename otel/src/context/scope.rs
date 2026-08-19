use crate::{
    context::{
        context::{ContextClassEntity, get_instance_id},
        storage,
    },
    trace::local_root_span,
};
use opentelemetry::Context;
use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::ReturnType,
    types::ReturnTypeHint,
};
use std::{convert::Infallible, sync::Arc};

const SCOPE_CLASS_NAME: &str = r"OpenTelemetry\Context\Scope";
#[derive(Default)]
pub struct ScopeState {
    pub context: Option<Arc<Context>>,
    detached: bool,
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
    let _scope_class = class.bound_class();
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
                let status = storage::detach_context(instance_id);
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
}
