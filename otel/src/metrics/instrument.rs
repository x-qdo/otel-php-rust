use crate::util;
use opentelemetry::{KeyValue, metrics};
use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};
use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex},
};

pub const COUNTER_CLASS_NAME: &str = r"OpenTelemetry\API\Metrics\Counter";
pub const UP_DOWN_COUNTER_CLASS_NAME: &str = r"OpenTelemetry\API\Metrics\UpDownCounter";
pub const HISTOGRAM_CLASS_NAME: &str = r"OpenTelemetry\API\Metrics\Histogram";
pub const GAUGE_CLASS_NAME: &str = r"OpenTelemetry\API\Metrics\Gauge";
pub const OBSERVABLE_COUNTER_CLASS_NAME: &str = r"OpenTelemetry\API\Metrics\ObservableCounter";
pub const OBSERVABLE_UP_DOWN_COUNTER_CLASS_NAME: &str =
    r"OpenTelemetry\API\Metrics\ObservableUpDownCounter";
pub const OBSERVABLE_GAUGE_CLASS_NAME: &str = r"OpenTelemetry\API\Metrics\ObservableGauge";

#[derive(Default)]
pub enum SynchronousInstrumentState {
    Counter(metrics::Counter<f64>, bool),
    UpDownCounter(metrics::UpDownCounter<f64>, bool),
    Histogram(metrics::Histogram<f64>, bool),
    Gauge(metrics::Gauge<f64>, bool),
    #[default]
    Empty,
}

pub type SynchronousInstrumentClass = StateClass<SynchronousInstrumentState>;

#[derive(Clone, Debug)]
pub struct Observation {
    pub value: f64,
    pub attributes: Vec<KeyValue>,
}

#[derive(Default, Debug)]
pub struct AsyncCache {
    observations: Mutex<HashMap<u64, Vec<Observation>>>,
}

impl AsyncCache {
    pub fn replace(&self, registration_id: u64, observations: Vec<Observation>) {
        self.observations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(registration_id, observations);
    }

    pub fn remove(&self, registration_id: u64) {
        self.observations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&registration_id);
    }

    pub fn push(&self, registration_id: u64, observation: Observation) {
        self.observations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(registration_id)
            .or_default()
            .push(observation);
    }

    fn snapshot(&self) -> Vec<Observation> {
        self.observations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .flatten()
            .cloned()
            .collect()
    }
}

#[derive(Default)]
pub struct AsynchronousInstrumentState {
    pub cache: Option<Arc<AsyncCache>>,
    pub enabled: bool,
    pub provider_key: Option<crate::metrics::meter_provider::ProviderKey>,
    pub scope_key: String,
}

pub type AsynchronousInstrumentClass = StateClass<AsynchronousInstrumentState>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AsyncKind {
    Counter,
    UpDownCounter,
    Gauge,
}

fn numeric_value(value: &ZVal) -> phper::Result<f64> {
    if let Some(value) = value.as_double() {
        return Ok(value);
    }
    if let Some(value) = value.as_long() {
        return Ok(value as f64);
    }
    value.expect_double()
}

fn attributes(arguments: &[ZVal], index: usize) -> phper::Result<Vec<KeyValue>> {
    match arguments.get(index) {
        Some(value) => {
            util::zval_iterable_to_key_value_vec(value, util::AttributeDestination::Metric)
        }
        None => Ok(Vec::new()),
    }
}

fn recording_arguments(typed_amount: bool) -> [Argument; 3] {
    let amount = if typed_amount {
        Argument::new("amount").with_type_hint(ArgumentTypeHint::Union(vec![
            ArgumentTypeHint::Float,
            ArgumentTypeHint::Int,
        ]))
    } else {
        Argument::new("amount")
    };
    let context = if typed_amount {
        Argument::new("context")
            .with_type_hint(ArgumentTypeHint::Union(vec![
                ArgumentTypeHint::ClassEntry(r"OpenTelemetry\Context\ContextInterface".to_string()),
                ArgumentTypeHint::False,
            ]))
            .allow_null()
            .with_default_value("NULL")
    } else {
        Argument::new("context").with_default_value("NULL")
    };
    [
        amount,
        Argument::new("attributes")
            .with_type_hint(ArgumentTypeHint::Iterable)
            .with_default_value("[]"),
        context,
    ]
}

fn instrument_enabled(state_enabled: bool) -> bool {
    state_enabled
}

fn make_synchronous_class(
    class_name: &str,
    operation: &str,
    interface: Interface,
    typed_amount: bool,
) -> ClassEntity<SynchronousInstrumentState> {
    let mut class: ClassEntity<SynchronousInstrumentState> =
        ClassEntity::new_with_default_state_constructor(class_name);
    class.set_final();
    class.implements(interface);
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method(operation, Visibility::Public, |this, arguments| {
            let amount = numeric_value(util::arg(arguments, 0)?)?;
            let attributes = attributes(arguments, 1)?;
            match this.as_state() {
                SynchronousInstrumentState::Counter(instrument, enabled) if *enabled => {
                    if amount >= 0.0 {
                        instrument.add(amount, &attributes);
                    } else {
                        tracing::warn!(
                            "negative value passed to a monotonic counter; measurement dropped"
                        );
                    }
                }
                SynchronousInstrumentState::UpDownCounter(instrument, enabled) if *enabled => {
                    instrument.add(amount, &attributes);
                }
                SynchronousInstrumentState::Histogram(instrument, enabled) if *enabled => {
                    if amount >= 0.0 {
                        instrument.record(amount, &attributes);
                    } else {
                        tracing::warn!("negative value passed to a histogram; measurement dropped");
                    }
                }
                SynchronousInstrumentState::Gauge(instrument, enabled) if *enabled => {
                    instrument.record(amount, &attributes);
                }
                _ => {}
            }
            Ok::<_, phper::Error>(())
        })
        .arguments(recording_arguments(typed_amount))
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    class
        .add_method("isEnabled", Visibility::Public, |this, _| {
            let enabled = match this.as_state() {
                SynchronousInstrumentState::Counter(_, enabled)
                | SynchronousInstrumentState::UpDownCounter(_, enabled)
                | SynchronousInstrumentState::Histogram(_, enabled)
                | SynchronousInstrumentState::Gauge(_, enabled) => *enabled,
                SynchronousInstrumentState::Empty => false,
            };
            Ok::<_, Infallible>(instrument_enabled(enabled))
        })
        .return_type(ReturnType::new(ReturnTypeHint::Bool));

    class
}

pub fn make_counter_class(interface: Interface) -> ClassEntity<SynchronousInstrumentState> {
    make_synchronous_class(COUNTER_CLASS_NAME, "add", interface, true)
}

pub fn make_up_down_counter_class(interface: Interface) -> ClassEntity<SynchronousInstrumentState> {
    make_synchronous_class(UP_DOWN_COUNTER_CLASS_NAME, "add", interface, false)
}

pub fn make_histogram_class(interface: Interface) -> ClassEntity<SynchronousInstrumentState> {
    make_synchronous_class(HISTOGRAM_CLASS_NAME, "record", interface, true)
}

pub fn make_gauge_class(interface: Interface) -> ClassEntity<SynchronousInstrumentState> {
    make_synchronous_class(GAUGE_CLASS_NAME, "record", interface, true)
}

pub fn build_async_cache(
    meter: &metrics::Meter,
    kind: AsyncKind,
    name: String,
    unit: Option<String>,
    description: Option<String>,
) -> Arc<AsyncCache> {
    let cache = Arc::new(AsyncCache::default());
    match kind {
        AsyncKind::Counter => {
            let callback_cache = cache.clone();
            let mut builder = meter
                .f64_observable_counter(name)
                .with_callback(move |observer| {
                    for observation in callback_cache.snapshot() {
                        observer.observe(observation.value, &observation.attributes);
                    }
                });
            if let Some(unit) = unit {
                builder = builder.with_unit(unit);
            }
            if let Some(description) = description {
                builder = builder.with_description(description);
            }
            let _instrument = builder.build();
        }
        AsyncKind::UpDownCounter => {
            let callback_cache = cache.clone();
            let mut builder =
                meter
                    .f64_observable_up_down_counter(name)
                    .with_callback(move |observer| {
                        for observation in callback_cache.snapshot() {
                            observer.observe(observation.value, &observation.attributes);
                        }
                    });
            if let Some(unit) = unit {
                builder = builder.with_unit(unit);
            }
            if let Some(description) = description {
                builder = builder.with_description(description);
            }
            let _instrument = builder.build();
        }
        AsyncKind::Gauge => {
            let callback_cache = cache.clone();
            let mut builder = meter
                .f64_observable_gauge(name)
                .with_callback(move |observer| {
                    for observation in callback_cache.snapshot() {
                        observer.observe(observation.value, &observation.attributes);
                    }
                });
            if let Some(unit) = unit {
                builder = builder.with_unit(unit);
            }
            if let Some(description) = description {
                builder = builder.with_description(description);
            }
            let _instrument = builder.build();
        }
    }
    cache
}

pub fn make_asynchronous_class(
    class_name: &str,
    interface: Interface,
    callback_class: crate::metrics::observable::ObservableCallbackClass,
    observer_class: crate::metrics::observable::ObserverClass,
) -> ClassEntity<AsynchronousInstrumentState> {
    let mut class: ClassEntity<AsynchronousInstrumentState> =
        ClassEntity::new_with_default_state_constructor(class_name);
    class.set_final();
    class.implements(interface);
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method("isEnabled", Visibility::Public, |this, _| {
            Ok::<_, Infallible>(instrument_enabled(this.as_state().enabled))
        })
        .return_type(ReturnType::new(ReturnTypeHint::Bool));

    class
        .add_method("observe", Visibility::Public, move |this, arguments| {
            if !this.as_state().enabled {
                return crate::metrics::observable::noop_callback(&callback_class);
            }
            let callback = util::arg(arguments, 0)?.clone();
            let cache =
                this.as_state().cache.clone().ok_or_else(|| {
                    phper::Error::boxed("observable instrument is not initialized")
                })?;
            crate::metrics::observable::register_callback(
                callback,
                vec![cache],
                &observer_class,
                &callback_class,
            )
        })
        .argument(Argument::new("callback").with_type_hint(ArgumentTypeHint::Callable))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\API\Metrics\ObservableCallbackInterface".to_string(),
        )));

    class
}
