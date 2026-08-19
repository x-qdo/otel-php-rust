use crate::{
    metrics::{
        instrument::{
            self, AsyncCache, AsyncKind, AsynchronousInstrumentClass, AsynchronousInstrumentState,
            SynchronousInstrumentClass, SynchronousInstrumentState,
        },
        meter_provider::ProviderKey,
        observable::{ObservableCallbackClass, ObserverClass},
    },
    util,
};
use once_cell::sync::Lazy;
use opentelemetry::metrics::Meter;
use phper::{
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    objects::{StateObj, StateObject},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};
use std::{
    collections::HashMap,
    convert::Infallible,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
};

const METER_CLASS_NAME: &str = r"OpenTelemetry\API\Metrics\Meter";

#[derive(Default)]
pub struct MeterState {
    pub meter: Option<Meter>,
    pub enabled: bool,
    pub provider_key: ProviderKey,
    pub scope_key: String,
}

pub type MeterClass = StateClass<MeterState>;

#[derive(Clone, Debug, Eq)]
struct AsyncInstrumentKey {
    provider_key: ProviderKey,
    scope_key: String,
    kind: AsyncKind,
    name: String,
    unit: Option<String>,
    description: Option<String>,
}

impl PartialEq for AsyncInstrumentKey {
    fn eq(&self, other: &Self) -> bool {
        self.provider_key == other.provider_key
            && self.scope_key == other.scope_key
            && self.kind == other.kind
            && self.name == other.name
            && self.unit == other.unit
            && self.description == other.description
    }
}

impl Hash for AsyncInstrumentKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.provider_key.hash(state);
        self.scope_key.hash(state);
        self.kind.hash(state);
        self.name.hash(state);
        self.unit.hash(state);
        self.description.hash(state);
    }
}

static ASYNC_CACHES: Lazy<Mutex<HashMap<AsyncInstrumentKey, Arc<AsyncCache>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn remove_async_caches(pid: u32) {
    ASYNC_CACHES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .retain(|key, _| key.provider_key.0 != pid);
}

fn optional_string(arguments: &[ZVal], index: usize) -> Option<String> {
    arguments
        .get(index)
        .and_then(|value| value.as_z_str())
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn configured_builder<T>(
    mut builder: opentelemetry::metrics::InstrumentBuilder<'_, T>,
    unit: Option<String>,
    description: Option<String>,
) -> opentelemetry::metrics::InstrumentBuilder<'_, T> {
    if let Some(unit) = unit {
        builder = builder.with_unit(unit);
    }
    if let Some(description) = description {
        builder = builder.with_description(description);
    }
    builder
}

fn factory_arguments(observable: bool) -> Vec<Argument> {
    let mut arguments = vec![
        Argument::new("name").with_type_hint(ArgumentTypeHint::String),
        Argument::new("unit")
            .with_type_hint(ArgumentTypeHint::String)
            .allow_null()
            .with_default_value("NULL"),
        Argument::new("description")
            .with_type_hint(ArgumentTypeHint::String)
            .allow_null()
            .with_default_value("NULL"),
    ];
    if observable {
        arguments.push(
            Argument::new("advisory")
                .with_type_hint(ArgumentTypeHint::Union(vec![
                    ArgumentTypeHint::Array,
                    ArgumentTypeHint::Callable,
                ]))
                .with_default_value("[]"),
        );
        arguments.push(
            Argument::new("callbacks")
                .with_type_hint(ArgumentTypeHint::Callable)
                .variadic(),
        );
    } else {
        arguments.push(
            Argument::new("advisory")
                .with_type_hint(ArgumentTypeHint::Array)
                .with_default_value("[]"),
        );
    }
    arguments
}

fn instrument_name(arguments: &[ZVal]) -> phper::Result<String> {
    Ok(util::arg(arguments, 0)?
        .expect_z_str()?
        .to_str()?
        .to_string())
}

fn create_async_cache(
    state: &MeterState,
    kind: AsyncKind,
    name: String,
    unit: Option<String>,
    description: Option<String>,
) -> phper::Result<Arc<AsyncCache>> {
    if !state.enabled {
        return Ok(Arc::new(AsyncCache::default()));
    }
    let meter = state
        .meter
        .as_ref()
        .ok_or_else(|| phper::Error::boxed("meter is not initialized"))?;
    let key = AsyncInstrumentKey {
        provider_key: state.provider_key.clone(),
        scope_key: state.scope_key.clone(),
        kind,
        name: name.clone(),
        unit: unit.clone(),
        description: description.clone(),
    };
    let mut caches = ASYNC_CACHES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(caches
        .entry(key)
        .or_insert_with(|| instrument::build_async_cache(meter, kind, name, unit, description))
        .clone())
}

fn initialize_async_object(
    class: &AsynchronousInstrumentClass,
    state: &MeterState,
    cache: Arc<AsyncCache>,
) -> phper::Result<StateObject<AsynchronousInstrumentState>> {
    let mut object = class.init_object()?;
    object.as_mut_state().cache = Some(cache);
    object.as_mut_state().enabled = state.enabled;
    object.as_mut_state().provider_key = Some(state.provider_key.clone());
    object.as_mut_state().scope_key = state.scope_key.clone();
    Ok(object)
}

fn callback_arguments(arguments: &[ZVal]) -> Vec<ZVal> {
    let mut callbacks = Vec::new();
    if let Some(advisory) = arguments.get(3)
        && advisory.as_z_arr().is_none()
    {
        callbacks.push(advisory.clone());
    }
    callbacks.extend(arguments.iter().skip(4).cloned());
    callbacks
}

fn register_initial_callbacks(
    callbacks: Vec<ZVal>,
    cache: &Arc<AsyncCache>,
    observer_class: &ObserverClass,
    callback_class: &ObservableCallbackClass,
) -> phper::Result<()> {
    for callback in callbacks {
        let _token = crate::metrics::observable::register_callback(
            callback,
            vec![cache.clone()],
            observer_class,
            callback_class,
        )?;
        // Registration ownership is request-scoped even when the factory's
        // convenience callback form does not return the detachable token.
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn make_meter_class(
    interface: Interface,
    counter_class: SynchronousInstrumentClass,
    up_down_counter_class: SynchronousInstrumentClass,
    histogram_class: SynchronousInstrumentClass,
    gauge_class: SynchronousInstrumentClass,
    observable_counter_class: AsynchronousInstrumentClass,
    observable_up_down_counter_class: AsynchronousInstrumentClass,
    observable_gauge_class: AsynchronousInstrumentClass,
    observer_class: ObserverClass,
    callback_class: ObservableCallbackClass,
) -> ClassEntity<MeterState> {
    let mut class: ClassEntity<MeterState> =
        ClassEntity::new_with_default_state_constructor(METER_CLASS_NAME);
    class.set_final();
    class.implements(interface);
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method(
            "createCounter",
            Visibility::Public,
            move |this, arguments| {
                let meter = this
                    .as_state()
                    .meter
                    .as_ref()
                    .ok_or_else(|| phper::Error::boxed("meter is not initialized"))?;
                let builder: opentelemetry::metrics::InstrumentBuilder<
                    '_,
                    opentelemetry::metrics::Counter<f64>,
                > = configured_builder(
                    meter.f64_counter(instrument_name(arguments)?),
                    optional_string(arguments, 1),
                    optional_string(arguments, 2),
                );
                let mut object = counter_class.init_object()?;
                *object.as_mut_state() =
                    SynchronousInstrumentState::Counter(builder.build(), this.as_state().enabled);
                Ok::<_, phper::Error>(object)
            },
        )
        .arguments(factory_arguments(false))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\API\Metrics\CounterInterface".to_string(),
        )));

    class
        .add_method(
            "createUpDownCounter",
            Visibility::Public,
            move |this, arguments| {
                let meter = this
                    .as_state()
                    .meter
                    .as_ref()
                    .ok_or_else(|| phper::Error::boxed("meter is not initialized"))?;
                let builder: opentelemetry::metrics::InstrumentBuilder<
                    '_,
                    opentelemetry::metrics::UpDownCounter<f64>,
                > = configured_builder(
                    meter.f64_up_down_counter(instrument_name(arguments)?),
                    optional_string(arguments, 1),
                    optional_string(arguments, 2),
                );
                let mut object = up_down_counter_class.init_object()?;
                *object.as_mut_state() = SynchronousInstrumentState::UpDownCounter(
                    builder.build(),
                    this.as_state().enabled,
                );
                Ok::<_, phper::Error>(object)
            },
        )
        .arguments(factory_arguments(false))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\API\Metrics\UpDownCounterInterface".to_string(),
        )));

    class
        .add_method("createGauge", Visibility::Public, move |this, arguments| {
            let meter = this
                .as_state()
                .meter
                .as_ref()
                .ok_or_else(|| phper::Error::boxed("meter is not initialized"))?;
            let builder: opentelemetry::metrics::InstrumentBuilder<
                '_,
                opentelemetry::metrics::Gauge<f64>,
            > = configured_builder(
                meter.f64_gauge(instrument_name(arguments)?),
                optional_string(arguments, 1),
                optional_string(arguments, 2),
            );
            let mut object = gauge_class.init_object()?;
            *object.as_mut_state() =
                SynchronousInstrumentState::Gauge(builder.build(), this.as_state().enabled);
            Ok::<_, phper::Error>(object)
        })
        .arguments(factory_arguments(false))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\API\Metrics\GaugeInterface".to_string(),
        )));

    class
        .add_method(
            "createHistogram",
            Visibility::Public,
            move |this, arguments| {
                let meter = this
                    .as_state()
                    .meter
                    .as_ref()
                    .ok_or_else(|| phper::Error::boxed("meter is not initialized"))?;
                let mut builder = meter.f64_histogram(instrument_name(arguments)?);
                if let Some(unit) = optional_string(arguments, 1) {
                    builder = builder.with_unit(unit);
                }
                if let Some(description) = optional_string(arguments, 2) {
                    builder = builder.with_description(description);
                }
                if let Some(boundaries) = arguments
                    .get(3)
                    .and_then(|value| value.as_z_arr())
                    .and_then(|advisory| advisory.get("ExplicitBucketBoundaries"))
                    .and_then(|value| value.as_z_arr())
                {
                    let values = boundaries
                        .iter()
                        .filter_map(|(_, value)| {
                            value
                                .as_double()
                                .or_else(|| value.as_long().map(|v| v as f64))
                        })
                        .collect::<Vec<_>>();
                    builder = builder.with_boundaries(values);
                }
                let mut object = histogram_class.init_object()?;
                *object.as_mut_state() =
                    SynchronousInstrumentState::Histogram(builder.build(), this.as_state().enabled);
                Ok::<_, phper::Error>(object)
            },
        )
        .arguments(factory_arguments(false))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\API\Metrics\HistogramInterface".to_string(),
        )));

    add_async_factory(
        &mut class,
        "createObservableCounter",
        "ObservableCounterInterface",
        AsyncKind::Counter,
        observable_counter_class,
        observer_class.clone(),
        callback_class.clone(),
    );
    add_async_factory(
        &mut class,
        "createObservableUpDownCounter",
        "ObservableUpDownCounterInterface",
        AsyncKind::UpDownCounter,
        observable_up_down_counter_class,
        observer_class.clone(),
        callback_class.clone(),
    );
    add_async_factory(
        &mut class,
        "createObservableGauge",
        "ObservableGaugeInterface",
        AsyncKind::Gauge,
        observable_gauge_class,
        observer_class.clone(),
        callback_class.clone(),
    );

    class
        .add_method(
            "batchObserve",
            Visibility::Public,
            move |this, arguments| {
                if !this.as_state().enabled {
                    return crate::metrics::observable::noop_callback(&callback_class);
                }
                let callback = util::arg(arguments, 0)?.clone();
                let mut caches = Vec::with_capacity(arguments.len().saturating_sub(1));
                for instrument in arguments.iter().skip(1) {
                    let object = instrument.expect_z_obj()?;
                    let state = unsafe {
                        object
                            .as_state_obj::<AsynchronousInstrumentState>()
                            .as_state()
                    };
                    if state.provider_key.as_ref() != Some(&this.as_state().provider_key)
                        || state.scope_key != this.as_state().scope_key
                    {
                        return Err(phper::Error::boxed(
                            "batchObserve instruments must be created by this meter",
                        ));
                    }
                    caches.push(state.cache.clone().ok_or_else(|| {
                        phper::Error::boxed("observable instrument is not initialized")
                    })?);
                }
                crate::metrics::observable::register_callback(
                    callback,
                    caches,
                    &observer_class,
                    &callback_class,
                )
            },
        )
        .argument(Argument::new("callback").with_type_hint(ArgumentTypeHint::Callable))
        .argument(
            Argument::new("instrument").with_type_hint(ArgumentTypeHint::ClassEntry(
                r"OpenTelemetry\API\Metrics\AsynchronousInstrument".to_string(),
            )),
        )
        .argument(
            Argument::new("instruments")
                .with_type_hint(ArgumentTypeHint::ClassEntry(
                    r"OpenTelemetry\API\Metrics\AsynchronousInstrument".to_string(),
                ))
                .variadic(),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\API\Metrics\ObservableCallbackInterface".to_string(),
        )));

    class
}

fn add_async_factory(
    class: &mut ClassEntity<MeterState>,
    method: &str,
    result_interface: &str,
    kind: AsyncKind,
    instrument_class: AsynchronousInstrumentClass,
    observer_class: ObserverClass,
    callback_class: ObservableCallbackClass,
) {
    class
        .add_method(
            method,
            Visibility::Public,
            move |this: &mut StateObj<MeterState>, arguments| {
                let name = instrument_name(arguments)?;
                let cache = create_async_cache(
                    this.as_state(),
                    kind,
                    name,
                    optional_string(arguments, 1),
                    optional_string(arguments, 2),
                )?;
                if this.as_state().enabled {
                    register_initial_callbacks(
                        callback_arguments(arguments),
                        &cache,
                        &observer_class,
                        &callback_class,
                    )?;
                }
                initialize_async_object(&instrument_class, this.as_state(), cache)
            },
        )
        .arguments(factory_arguments(true))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(format!(
            r"OpenTelemetry\API\Metrics\{result_interface}"
        ))));
}
