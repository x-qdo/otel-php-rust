use crate::{
    baggage::propagation::baggage_propagator::BaggagePropagatorClass,
    context::{
        context_class::{ContextClass, current_context_value},
        propagation::{
            multi_text_map_propagator::MultiTextMapPropagatorClass,
            native_noop_response_propagator::NativeNoopResponsePropagatorClass,
        },
    },
    logs::logger_provider::LoggerProviderClass,
    metrics::meter_provider::MeterProviderClass,
    trace::{
        propagation::trace_context_propagator::TraceContextPropagatorClass,
        tracer_provider::TracerProviderClass,
    },
};
use phper::{
    arrays::ZArray,
    classes::{ClassEntity, ClassEntry, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
    values::ZVal,
};
use std::rc::Rc;

const GLOBALS_CLASS_NAME: &str = r"OpenTelemetry\API\Globals";
const CONFIGURATOR_CLASS: &str = r"OpenTelemetry\API\Instrumentation\Configurator";
const CONTEXT_KEYS_CLASS: &str = r"OpenTelemetry\API\Instrumentation\ContextKeys";

const TRACER_PROVIDER_INTERFACE: &str = r"OpenTelemetry\API\Trace\TracerProviderInterface";
const METER_PROVIDER_INTERFACE: &str = r"OpenTelemetry\API\Metrics\MeterProviderInterface";
const LOGGER_PROVIDER_INTERFACE: &str = r"OpenTelemetry\API\Logs\LoggerProviderInterface";
const EVENT_LOGGER_PROVIDER_INTERFACE: &str =
    r"OpenTelemetry\API\Logs\EventLoggerProviderInterface";
const TEXT_MAP_PROPAGATOR_INTERFACE: &str =
    r"OpenTelemetry\Context\Propagation\TextMapPropagatorInterface";
const RESPONSE_PROPAGATOR_INTERFACE: &str =
    r"OpenTelemetry\Context\Propagation\ResponsePropagatorInterface";

type GlobalsClass = StateClass<GlobalsState>;

#[derive(Clone, Default)]
pub struct GlobalsState {
    tracer_provider: ZVal,
    meter_provider: ZVal,
    logger_provider: ZVal,
    event_logger_provider: ZVal,
    propagator: ZVal,
    response_propagator: ZVal,
}

#[derive(Clone)]
struct GlobalsDependencies {
    tracer_provider_class: TracerProviderClass,
    meter_provider_class: MeterProviderClass,
    logger_provider_class: LoggerProviderClass,
    trace_context_propagator_class: TraceContextPropagatorClass,
    baggage_propagator_class: BaggagePropagatorClass,
    multi_text_map_propagator_class: MultiTextMapPropagatorClass,
    response_propagator_class: NativeNoopResponsePropagatorClass,
    context_class: ContextClass,
}

fn class_exists(name: &str) -> phper::Result<bool> {
    if ClassEntry::from_globals(name).is_ok() {
        return Ok(true);
    }
    Ok(
        phper::functions::call("class_exists", &mut [ZVal::from(name), ZVal::from(true)])?
            .as_bool()
            .unwrap_or(false),
    )
}

fn static_call(class: &str, method: &str, arguments: &mut [ZVal]) -> phper::Result<ZVal> {
    let mut callable = ZArray::new();
    callable.insert((), class);
    callable.insert((), method);
    phper::functions::call(ZVal::from(callable), arguments)
}

fn native_defaults(dependencies: &GlobalsDependencies) -> phper::Result<GlobalsState> {
    let tracer_provider = ZVal::from(dependencies.tracer_provider_class.init_object()?);
    let meter_provider = ZVal::from(dependencies.meter_provider_class.init_object()?);
    let logger_provider = ZVal::from(dependencies.logger_provider_class.init_object()?);
    let event_logger_provider = logger_provider.clone();
    let trace = ZVal::from(dependencies.trace_context_propagator_class.init_object()?);
    let baggage = ZVal::from(dependencies.baggage_propagator_class.init_object()?);
    let mut propagators = ZArray::new();
    propagators.insert((), trace);
    propagators.insert((), baggage);
    let propagator = ZVal::from(
        dependencies
            .multi_text_map_propagator_class
            .new_object([ZVal::from(propagators)])?,
    );
    let response_propagator = ZVal::from(dependencies.response_propagator_class.init_object()?);

    Ok(GlobalsState {
        tracer_provider,
        meter_provider,
        logger_provider,
        event_logger_provider,
        propagator,
        response_propagator,
    })
}

fn configure_native_defaults(
    mut configurator: ZVal,
    defaults: &GlobalsState,
) -> phper::Result<ZVal> {
    for (method, value) in [
        ("withTracerProvider", &defaults.tracer_provider),
        ("withMeterProvider", &defaults.meter_provider),
        ("withLoggerProvider", &defaults.logger_provider),
        ("withEventLoggerProvider", &defaults.event_logger_provider),
        ("withPropagator", &defaults.propagator),
        ("withResponsePropagator", &defaults.response_propagator),
    ] {
        configurator = configurator
            .expect_mut_z_obj()?
            .call(method, &mut [value.clone()])?;
    }
    Ok(configurator)
}

fn context_key(method: &str) -> phper::Result<ZVal> {
    static_call(CONTEXT_KEYS_CLASS, method, &mut [])
}

fn context_value(context: &mut ZVal, method: &str) -> phper::Result<ZVal> {
    let key = context_key(method)?;
    context.expect_mut_z_obj()?.call("get", &mut [key])
}

fn apply_initializers(owner: &GlobalsClass, defaults: GlobalsState) -> phper::Result<GlobalsState> {
    let Some(initializers) = owner
        .as_class_entry()
        .get_static_property("initializers")
        .filter(|value| value.as_z_arr().is_some())
        .cloned()
    else {
        return Ok(defaults);
    };
    if initializers.expect_z_arr()?.iter().next().is_none() || !class_exists(CONFIGURATOR_CLASS)? {
        return Ok(defaults);
    }

    let configurator = static_call(CONFIGURATOR_CLASS, "create", &mut [])?;
    let mut configurator = configure_native_defaults(configurator, &defaults)?;
    let mut scope = configurator.expect_mut_z_obj()?.call("activate", [])?;
    let configured = (|| -> phper::Result<ZVal> {
        for (_, initializer) in initializers.expect_z_arr()?.iter() {
            let mut initializer = initializer.clone();
            match initializer.call(&mut [configurator.clone()]) {
                Ok(configured) => configurator = configured,
                Err(error) => tracing::warn!("Error during OpenTelemetry initialization: {error}"),
            }
        }
        Ok(configurator)
    })();
    let detached = scope.expect_mut_z_obj()?.call("detach", []);
    let mut configurator = configured?;
    detached?;

    let mut context = configurator
        .expect_mut_z_obj()?
        .call("storeInContext", [])?;
    Ok(GlobalsState {
        tracer_provider: context_value(&mut context, "tracerProvider")?,
        meter_provider: context_value(&mut context, "meterProvider")?,
        logger_provider: context_value(&mut context, "loggerProvider")?,
        event_logger_provider: context_value(&mut context, "eventLoggerProvider")?,
        propagator: context_value(&mut context, "propagator")?,
        response_propagator: context_value(&mut context, "responsePropagator")?,
    })
}

fn globals_instance(
    owner: &GlobalsClass,
    dependencies: &GlobalsDependencies,
) -> phper::Result<ZVal> {
    if let Some(globals) = owner
        .as_class_entry()
        .get_static_property("globals")
        .filter(|value| value.as_z_obj().is_some())
    {
        return Ok(globals.clone());
    }
    let state = apply_initializers(owner, native_defaults(dependencies)?)?;
    let mut object = owner.init_object()?;
    *object.as_mut_state() = state;
    let globals = ZVal::from(object);
    owner
        .as_class_entry()
        .set_static_property("globals", globals.clone());
    Ok(globals)
}

fn default_value(
    owner: &GlobalsClass,
    dependencies: &GlobalsDependencies,
    select: impl FnOnce(&GlobalsState) -> &ZVal,
) -> phper::Result<ZVal> {
    let globals = globals_instance(owner, dependencies)?;
    let object = globals.expect_z_obj()?;
    let state = unsafe { object.as_state_obj::<GlobalsState>() };
    Ok(select(state.as_state()).clone())
}

fn contextual_value(
    owner: &GlobalsClass,
    dependencies: &GlobalsDependencies,
    context_method: &str,
    select: impl FnOnce(&GlobalsState) -> &ZVal,
) -> phper::Result<ZVal> {
    if class_exists(CONTEXT_KEYS_CLASS)? {
        let key = context_key(context_method)?;
        let mut current = current_context_value(&dependencies.context_class)?;
        let value = current.expect_mut_z_obj()?.call("get", &mut [key])?;
        if !value.get_type_info().is_null() {
            return Ok(value);
        }
    }
    default_value(owner, dependencies, select)
}

#[allow(clippy::too_many_arguments)]
pub fn make_globals_class(
    tracer_provider_class: TracerProviderClass,
    meter_provider_class: MeterProviderClass,
    logger_provider_class: LoggerProviderClass,
    trace_context_propagator_class: TraceContextPropagatorClass,
    baggage_propagator_class: BaggagePropagatorClass,
    multi_text_map_propagator_class: MultiTextMapPropagatorClass,
    response_propagator_class: NativeNoopResponsePropagatorClass,
    context_class: ContextClass,
) -> ClassEntity<GlobalsState> {
    let mut class = ClassEntity::new_with_default_state_constructor(GLOBALS_CLASS_NAME);
    class.set_final();
    class.state_cloner(Clone::clone);
    class.add_static_property("initializers", Visibility::Private, ());
    class.add_static_property("globals", Visibility::Private, ());
    let owner = class.bound_class();
    let dependencies = Rc::new(GlobalsDependencies {
        tracer_provider_class,
        meter_provider_class,
        logger_provider_class,
        trace_context_propagator_class,
        baggage_propagator_class,
        multi_text_map_propagator_class,
        response_propagator_class,
        context_class,
    });

    class
        .add_method("__construct", Visibility::Public, |this, arguments| {
            *this.as_mut_state() = GlobalsState {
                tracer_provider: crate::util::arg(arguments, 0)?.clone(),
                meter_provider: crate::util::arg(arguments, 1)?.clone(),
                logger_provider: crate::util::arg(arguments, 2)?.clone(),
                event_logger_provider: crate::util::arg(arguments, 3)?.clone(),
                propagator: crate::util::arg(arguments, 4)?.clone(),
                response_propagator: crate::util::arg(arguments, 5)?.clone(),
            };
            Ok::<_, phper::Error>(())
        })
        .argument(
            Argument::new("tracerProvider").with_type_hint(ArgumentTypeHint::ClassEntry(
                TRACER_PROVIDER_INTERFACE.to_string(),
            )),
        )
        .argument(
            Argument::new("meterProvider").with_type_hint(ArgumentTypeHint::ClassEntry(
                METER_PROVIDER_INTERFACE.to_string(),
            )),
        )
        .argument(
            Argument::new("loggerProvider").with_type_hint(ArgumentTypeHint::ClassEntry(
                LOGGER_PROVIDER_INTERFACE.to_string(),
            )),
        )
        .argument(Argument::new("eventLoggerProvider").with_type_hint(
            ArgumentTypeHint::ClassEntry(EVENT_LOGGER_PROVIDER_INTERFACE.to_string()),
        ))
        .argument(
            Argument::new("propagator").with_type_hint(ArgumentTypeHint::ClassEntry(
                TEXT_MAP_PROPAGATOR_INTERFACE.to_string(),
            )),
        )
        .argument(Argument::new("responsePropagator").with_type_hint(
            ArgumentTypeHint::ClassEntry(RESPONSE_PROPAGATOR_INTERFACE.to_string()),
        ));

    let tracer_owner = owner.clone();
    let tracer_dependencies = dependencies.clone();
    class
        .add_static_method("tracerProvider", Visibility::Public, move |_| {
            contextual_value(
                &tracer_owner,
                &tracer_dependencies,
                "tracerProvider",
                |state| &state.tracer_provider,
            )
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            TRACER_PROVIDER_INTERFACE.to_string(),
        )));

    let meter_owner = owner.clone();
    let meter_dependencies = dependencies.clone();
    class
        .add_static_method("meterProvider", Visibility::Public, move |_| {
            contextual_value(
                &meter_owner,
                &meter_dependencies,
                "meterProvider",
                |state| &state.meter_provider,
            )
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            METER_PROVIDER_INTERFACE.to_string(),
        )));

    let logger_owner = owner.clone();
    let logger_dependencies = dependencies.clone();
    class
        .add_static_method("loggerProvider", Visibility::Public, move |_| {
            contextual_value(
                &logger_owner,
                &logger_dependencies,
                "loggerProvider",
                |state| &state.logger_provider,
            )
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            LOGGER_PROVIDER_INTERFACE.to_string(),
        )));

    let event_owner = owner.clone();
    let event_dependencies = dependencies.clone();
    class
        .add_static_method("eventLoggerProvider", Visibility::Public, move |_| {
            contextual_value(
                &event_owner,
                &event_dependencies,
                "eventLoggerProvider",
                |state| &state.event_logger_provider,
            )
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            EVENT_LOGGER_PROVIDER_INTERFACE.to_string(),
        )));

    let propagator_owner = owner.clone();
    let propagator_dependencies = dependencies.clone();
    class
        .add_static_method("propagator", Visibility::Public, move |_| {
            contextual_value(
                &propagator_owner,
                &propagator_dependencies,
                "propagator",
                |state| &state.propagator,
            )
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            TEXT_MAP_PROPAGATOR_INTERFACE.to_string(),
        )));

    let response_owner = owner.clone();
    let response_dependencies = dependencies;
    class
        .add_static_method("responsePropagator", Visibility::Public, move |_| {
            contextual_value(
                &response_owner,
                &response_dependencies,
                "responsePropagator",
                |state| &state.response_propagator,
            )
        })
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            RESPONSE_PROPAGATOR_INTERFACE.to_string(),
        )));

    let initializer_owner = owner.clone();
    class
        .add_static_method(
            "registerInitializer",
            Visibility::Public,
            move |arguments| {
                let mut initializers = initializer_owner
                    .as_class_entry()
                    .get_static_property("initializers")
                    .filter(|value| value.as_z_arr().is_some())
                    .cloned()
                    .unwrap_or_else(|| ZVal::from(ZArray::new()));
                initializers
                    .expect_mut_z_arr()?
                    .insert((), crate::util::arg(arguments, 0)?.clone());
                initializer_owner
                    .as_class_entry()
                    .set_static_property("initializers", initializers);
                Ok::<_, phper::Error>(())
            },
        )
        .argument(
            Argument::new("initializer")
                .with_type_hint(ArgumentTypeHint::ClassEntry("Closure".to_string())),
        )
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    class
        .add_static_method("reset", Visibility::Public, move |_| {
            owner
                .as_class_entry()
                .set_static_property("globals", ZVal::default());
            owner
                .as_class_entry()
                .set_static_property("initializers", ZVal::default());
            Ok::<_, std::convert::Infallible>(())
        })
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    for method in ["logDebug", "logInfo", "logNotice", "logWarning", "logError"] {
        class
            .add_static_method(method, Visibility::Protected, |_| {
                Ok::<_, std::convert::Infallible>(())
            })
            .argument(Argument::new("message").with_type_hint(ArgumentTypeHint::String))
            .argument(
                Argument::new("context")
                    .with_type_hint(ArgumentTypeHint::Array)
                    .with_default_value("[]"),
            )
            .return_type(ReturnType::new(ReturnTypeHint::Void));
    }

    class
}
