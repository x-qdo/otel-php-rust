use crate::{
    metrics::instrument::{AsyncCache, Observation},
    util,
};
use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    objects::StateObject,
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

const OBSERVER_CLASS_NAME: &str = r"OpenTelemetry\API\Metrics\Observer";
const CALLBACK_CLASS_NAME: &str = r"OpenTelemetry\API\Metrics\ObservableCallback";

#[derive(Default)]
pub struct ObserverState {
    cache: Option<Arc<AsyncCache>>,
    registration_id: u64,
}

#[derive(Default)]
pub struct ObservableCallbackState {
    registration_id: Option<u64>,
}

pub type ObserverClass = StateClass<ObserverState>;
pub type ObservableCallbackClass = StateClass<ObservableCallbackState>;

#[derive(Clone)]
struct CallbackTarget {
    cache: Arc<AsyncCache>,
    registration_id: u64,
}

struct CallbackRegistration {
    callback: ZVal,
    targets: Vec<CallbackTarget>,
    observer_class: ObserverClass,
}

thread_local! {
    static CALLBACKS: RefCell<HashMap<u64, CallbackRegistration>> = RefCell::new(HashMap::new());
}

static NEXT_CALLBACK_ID: AtomicU64 = AtomicU64::new(1);

fn numeric_value(value: &ZVal) -> phper::Result<f64> {
    if let Some(value) = value.as_double() {
        return Ok(value);
    }
    if let Some(value) = value.as_long() {
        return Ok(value as f64);
    }
    value.expect_double()
}

pub fn make_observer_class(interface: Interface) -> ClassEntity<ObserverState> {
    let mut class: ClassEntity<ObserverState> =
        ClassEntity::new_with_default_state_constructor(OBSERVER_CLASS_NAME);
    class.set_final();
    class.implements(interface);
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });
    class
        .add_method("observe", Visibility::Public, |this, arguments| {
            let amount = numeric_value(util::arg(arguments, 0)?)?;
            let attributes = match arguments.get(1) {
                Some(attributes) => util::zval_iterable_to_key_value_vec(
                    attributes,
                    util::AttributeDestination::Metric,
                )?,
                None => Vec::new(),
            };
            let state = this.as_state();
            if let Some(cache) = &state.cache {
                cache.push(
                    state.registration_id,
                    Observation {
                        value: amount,
                        attributes,
                    },
                );
            }
            Ok::<_, phper::Error>(())
        })
        .argument(
            Argument::new("amount").with_type_hint(ArgumentTypeHint::Union(vec![
                ArgumentTypeHint::Float,
                ArgumentTypeHint::Int,
            ])),
        )
        .argument(
            Argument::new("attributes")
                .with_type_hint(ArgumentTypeHint::Iterable)
                .with_default_value("[]"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));
    class
}

pub fn make_observable_callback_class(
    interface: Interface,
) -> ClassEntity<ObservableCallbackState> {
    let mut class: ClassEntity<ObservableCallbackState> =
        ClassEntity::new_with_default_state_constructor(CALLBACK_CLASS_NAME);
    class.set_final();
    class.implements(interface);
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });
    class
        .add_method("detach", Visibility::Public, |this, _| {
            if let Some(registration_id) = this.as_mut_state().registration_id.take() {
                detach_callback(registration_id);
            }
            Ok::<_, Infallible>(())
        })
        .return_type(ReturnType::new(ReturnTypeHint::Void));
    class
}

fn invoke_callback(registration_id: u64) -> phper::Result<()> {
    let snapshot = CALLBACKS.with(|callbacks| {
        let callbacks = callbacks.borrow();
        callbacks.get(&registration_id).map(|registration| {
            (
                registration.callback.clone(),
                registration.targets.clone(),
                registration.observer_class.clone(),
            )
        })
    });
    let Some((mut callback, targets, observer_class)) = snapshot else {
        return Ok(());
    };

    for target in &targets {
        target.cache.replace(target.registration_id, Vec::new());
    }

    let mut arguments = Vec::with_capacity(targets.len());
    for target in targets {
        let mut observer = observer_class.init_object()?;
        observer.as_mut_state().cache = Some(target.cache);
        observer.as_mut_state().registration_id = target.registration_id;
        arguments.push(ZVal::from(observer));
    }
    callback.call(arguments.as_mut_slice())?;
    Ok(())
}

pub fn register_callback(
    callback: ZVal,
    caches: Vec<Arc<AsyncCache>>,
    observer_class: &ObserverClass,
    callback_class: &ObservableCallbackClass,
) -> phper::Result<StateObject<ObservableCallbackState>> {
    let registration_id = NEXT_CALLBACK_ID.fetch_add(1, Ordering::Relaxed);
    let targets: Vec<_> = caches
        .into_iter()
        .map(|cache| CallbackTarget {
            cache,
            registration_id,
        })
        .collect();

    CALLBACKS.with(|callbacks| {
        callbacks.borrow_mut().insert(
            registration_id,
            CallbackRegistration {
                callback,
                targets,
                observer_class: observer_class.clone(),
            },
        );
    });

    if let Err(error) = invoke_callback(registration_id) {
        detach_callback(registration_id);
        return Err(error);
    }

    let mut token = callback_class.init_object()?;
    token.as_mut_state().registration_id = Some(registration_id);
    Ok(token)
}

pub fn noop_callback(
    callback_class: &ObservableCallbackClass,
) -> phper::Result<StateObject<ObservableCallbackState>> {
    callback_class.init_object()
}

pub fn detach_callback(registration_id: u64) {
    let registration = CALLBACKS.with(|callbacks| callbacks.borrow_mut().remove(&registration_id));
    if let Some(registration) = registration {
        for target in registration.targets {
            target.cache.remove(target.registration_id);
        }
    }
}

/// Refresh PHP observable callbacks on the request thread, flush their latest
/// values once, and release every request-owned zval before PHP tears the
/// request heap down. Exporter threads only ever read Rust-owned snapshots.
pub fn flush_and_clear_request_callbacks() {
    let ids = CALLBACKS.with(|callbacks| callbacks.borrow().keys().copied().collect::<Vec<_>>());
    if ids.is_empty() {
        return;
    }

    for id in &ids {
        if let Err(error) = invoke_callback(*id) {
            tracing::warn!("metrics observable callback failed at request shutdown: {error}");
        }
    }
    crate::metrics::meter_provider::force_flush();
    for id in ids {
        detach_callback(id);
    }
}
