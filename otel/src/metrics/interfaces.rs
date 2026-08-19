use phper::{
    classes::{Interface, InterfaceEntity},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};

const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";
const OBSERVABLE_CALLBACK_INTERFACE: &str =
    r"OpenTelemetry\API\Metrics\ObservableCallbackInterface";

fn numeric_argument(name: &str) -> Argument {
    Argument::new(name).with_type_hint(ArgumentTypeHint::Union(vec![
        ArgumentTypeHint::Float,
        ArgumentTypeHint::Int,
    ]))
}

fn attributes_argument() -> Argument {
    Argument::new("attributes")
        .with_type_hint(ArgumentTypeHint::Iterable)
        .with_default_value("[]")
}

fn context_argument() -> Argument {
    Argument::new("context")
        .with_type_hint(ArgumentTypeHint::Union(vec![
            ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string()),
            ArgumentTypeHint::False,
        ]))
        .allow_null()
        .with_default_value("NULL")
}

fn nullable_string_argument(name: &str) -> Argument {
    Argument::new(name)
        .with_type_hint(ArgumentTypeHint::String)
        .allow_null()
        .with_default_value("NULL")
}

fn add_is_enabled(interface: &mut InterfaceEntity) {
    interface
        .add_method("isEnabled")
        .return_type(ReturnType::new(ReturnTypeHint::Bool));
}

pub fn make_instrument_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(r"OpenTelemetry\API\Metrics\Instrument");
    add_is_enabled(&mut interface);
    interface
}

pub fn make_synchronous_instrument_interface(instrument: Interface) -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(r"OpenTelemetry\API\Metrics\SynchronousInstrument");
    interface.extends(instrument);
    interface
}

pub fn make_asynchronous_instrument_interface(instrument: Interface) -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(r"OpenTelemetry\API\Metrics\AsynchronousInstrument");
    interface.extends(instrument);
    interface
}

fn make_recording_interface(
    name: &str,
    operation: &str,
    synchronous: Interface,
    typed_amount: bool,
) -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(name);
    interface.extends(synchronous);
    let amount = if typed_amount {
        numeric_argument("amount")
    } else {
        Argument::new("amount")
    };
    let context = if typed_amount {
        context_argument()
    } else {
        Argument::new("context").with_default_value("NULL")
    };
    interface
        .add_method(operation)
        .argument(amount)
        .argument(attributes_argument())
        .argument(context)
        .return_type(ReturnType::new(ReturnTypeHint::Void));
    interface
}

pub fn make_counter_interface(synchronous: Interface) -> InterfaceEntity {
    make_recording_interface(
        r"OpenTelemetry\API\Metrics\CounterInterface",
        "add",
        synchronous,
        true,
    )
}

pub fn make_up_down_counter_interface(synchronous: Interface) -> InterfaceEntity {
    // The locked open-telemetry/api 1.10 contract intentionally leaves amount
    // and context untyped on UpDownCounterInterface for BC.
    make_recording_interface(
        r"OpenTelemetry\API\Metrics\UpDownCounterInterface",
        "add",
        synchronous,
        false,
    )
}

pub fn make_histogram_interface(synchronous: Interface) -> InterfaceEntity {
    make_recording_interface(
        r"OpenTelemetry\API\Metrics\HistogramInterface",
        "record",
        synchronous,
        true,
    )
}

pub fn make_gauge_interface(synchronous: Interface) -> InterfaceEntity {
    make_recording_interface(
        r"OpenTelemetry\API\Metrics\GaugeInterface",
        "record",
        synchronous,
        true,
    )
}

fn make_observable_interface(name: &str, asynchronous: Interface) -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(name);
    interface.extends(asynchronous);
    interface
        .add_method("observe")
        .argument(Argument::new("callback").with_type_hint(ArgumentTypeHint::Callable))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            OBSERVABLE_CALLBACK_INTERFACE.to_string(),
        )));
    interface
}

pub fn make_observable_counter_interface(asynchronous: Interface) -> InterfaceEntity {
    make_observable_interface(
        r"OpenTelemetry\API\Metrics\ObservableCounterInterface",
        asynchronous,
    )
}

pub fn make_observable_up_down_counter_interface(asynchronous: Interface) -> InterfaceEntity {
    make_observable_interface(
        r"OpenTelemetry\API\Metrics\ObservableUpDownCounterInterface",
        asynchronous,
    )
}

pub fn make_observable_gauge_interface(asynchronous: Interface) -> InterfaceEntity {
    make_observable_interface(
        r"OpenTelemetry\API\Metrics\ObservableGaugeInterface",
        asynchronous,
    )
}

pub fn make_observer_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(r"OpenTelemetry\API\Metrics\ObserverInterface");
    interface
        .add_method("observe")
        .argument(numeric_argument("amount"))
        .argument(attributes_argument())
        .return_type(ReturnType::new(ReturnTypeHint::Void));
    interface
}

pub fn make_observable_callback_interface() -> InterfaceEntity {
    let mut interface =
        InterfaceEntity::new(r"OpenTelemetry\API\Metrics\ObservableCallbackInterface");
    interface
        .add_method("detach")
        .return_type(ReturnType::new(ReturnTypeHint::Void));
    interface
}

pub fn make_meter_provider_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(r"OpenTelemetry\API\Metrics\MeterProviderInterface");
    interface
        .add_method("getMeter")
        .argument(Argument::new("name").with_type_hint(ArgumentTypeHint::String))
        .argument(nullable_string_argument("version"))
        .argument(nullable_string_argument("schemaUrl"))
        .argument(attributes_argument())
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            r"OpenTelemetry\API\Metrics\MeterInterface".to_string(),
        )));
    interface
}

pub fn make_meter_interface() -> InterfaceEntity {
    let mut interface = InterfaceEntity::new(r"OpenTelemetry\API\Metrics\MeterInterface");

    interface
        .add_method("batchObserve")
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
            OBSERVABLE_CALLBACK_INTERFACE.to_string(),
        )));

    add_sync_factory(&mut interface, "createCounter", "CounterInterface");
    add_observable_factory(
        &mut interface,
        "createObservableCounter",
        "ObservableCounterInterface",
    );
    add_sync_factory(&mut interface, "createHistogram", "HistogramInterface");
    add_sync_factory(&mut interface, "createGauge", "GaugeInterface");
    add_observable_factory(
        &mut interface,
        "createObservableGauge",
        "ObservableGaugeInterface",
    );
    add_sync_factory(
        &mut interface,
        "createUpDownCounter",
        "UpDownCounterInterface",
    );
    add_observable_factory(
        &mut interface,
        "createObservableUpDownCounter",
        "ObservableUpDownCounterInterface",
    );

    interface
}

fn add_sync_factory(interface: &mut InterfaceEntity, method: &str, result: &str) {
    interface
        .add_method(method)
        .argument(Argument::new("name").with_type_hint(ArgumentTypeHint::String))
        .argument(nullable_string_argument("unit"))
        .argument(nullable_string_argument("description"))
        .argument(
            Argument::new("advisory")
                .with_type_hint(ArgumentTypeHint::Array)
                .with_default_value("[]"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(format!(
            r"OpenTelemetry\API\Metrics\{result}"
        ))));
}

fn add_observable_factory(interface: &mut InterfaceEntity, method: &str, result: &str) {
    interface
        .add_method(method)
        .argument(Argument::new("name").with_type_hint(ArgumentTypeHint::String))
        .argument(nullable_string_argument("unit"))
        .argument(nullable_string_argument("description"))
        .argument(
            Argument::new("advisory")
                .with_type_hint(ArgumentTypeHint::Union(vec![
                    ArgumentTypeHint::Array,
                    ArgumentTypeHint::Callable,
                ]))
                .with_default_value("[]"),
        )
        .argument(
            Argument::new("callbacks")
                .with_type_hint(ArgumentTypeHint::Callable)
                .variadic(),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(format!(
            r"OpenTelemetry\API\Metrics\{result}"
        ))));
}
