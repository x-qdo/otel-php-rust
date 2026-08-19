use crate::{
    baggage::{
        baggage::make_baggage_class,
        baggage_builder::make_baggage_builder_class,
        entry::make_entry_class,
        interfaces::{
            make_baggage_builder_interface, make_baggage_interface, make_metadata_interface,
        },
        metadata::make_metadata_class,
        propagation::{
            baggage_propagator::make_baggage_propagator_class, parser::make_parser_class,
        },
    },
    context::{
        context_class::{build_context_class, new_context_class},
        context_interface::{make_context_interface, make_implicit_context_keyed_interface},
        context_key::{
            make_context_key_class, make_context_key_interface, make_context_keys_class,
        },
        context_storage_interface::{
            make_context_storage_interface, make_context_storage_scope_interface,
            make_execution_context_aware_interface,
        },
        propagation::{
            array_access_getter_setter::make_array_access_getter_setter_class,
            multi_text_map_propagator::make_multi_text_map_propagator_class,
            native_noop_response_propagator::make_native_noop_response_propagator_class,
            text_map_propagator_interface::{
                make_extended_propagation_getter_interface, make_propagation_getter_interface,
                make_propagation_setter_interface, make_response_propagator_interface,
                make_text_map_propagator_interface,
            },
        },
        scope::{build_scope_class, new_scope_class},
        scope_interface::make_scope_interface,
        storage::{build_storage_class, new_storage_class},
    },
    globals::make_globals_class,
    logs::{
        event_logger::make_event_logger_class,
        log_record::make_log_record_class,
        log_record_builder::make_log_record_builder_class,
        logger::make_logger_class,
        logger_interface::{
            make_event_logger_interface, make_log_record_builder_interface, make_logger_interface,
        },
        logger_provider::make_logger_provider_class,
        logger_provider_interface::{
            make_event_logger_provider_interface, make_logger_provider_interface,
        },
        memory_exporter::make_logs_memory_exporter_class,
        severity::make_severity_enum,
    },
    metrics::{
        instrument::{
            OBSERVABLE_COUNTER_CLASS_NAME, OBSERVABLE_GAUGE_CLASS_NAME,
            OBSERVABLE_UP_DOWN_COUNTER_CLASS_NAME, make_asynchronous_class, make_counter_class,
            make_gauge_class, make_histogram_class, make_up_down_counter_class,
        },
        interfaces,
        memory_exporter::make_memory_exporter_class as make_metrics_memory_exporter_class,
        meter::make_meter_class,
        meter_provider::make_meter_provider_class,
        observable::{make_observable_callback_class, make_observer_class},
    },
    signals::make_signals_interface,
    trace::{
        local_root_span::make_local_root_span_class, memory_exporter::make_memory_exporter_class,
        non_recording_span::make_non_recording_span_class,
        propagation::trace_context_propagator::make_trace_context_propagator_class,
        span::{make_span_base_class, make_span_class},
        span_builder::make_span_builder_class,
        span_builder_interface::make_span_builder_interface,
        span_context::make_span_context_class,
        span_context_interface::make_span_context_interface,
        span_interface::make_span_interface, span_kind::make_span_kind_interface,
        status_code::make_status_code_interface, trace_flags::make_trace_flags_interface,
        trace_state::{make_trace_state_class, make_trace_state_interface},
        tracer::make_tracer_class,
        tracer_interface::make_tracer_interface, tracer_provider::make_tracer_provider_class,
        tracer_provider_interface::make_tracer_provider_interface,
    },
};
use phper::modules::Module;

pub fn register_classes_and_interfaces(module: &mut Module) {
    // interfaces
    let scope_interface = module.add_interface(make_scope_interface());
    let context_key_interface = module.add_interface(make_context_key_interface());
    let implicit_context_keyed_interface =
        module.add_interface(make_implicit_context_keyed_interface());
    let metadata_interface = module.add_interface(make_metadata_interface());
    let baggage_builder_interface = module.add_interface(make_baggage_builder_interface());
    let baggage_interface = module.add_interface(make_baggage_interface(
        implicit_context_keyed_interface.clone(),
    ));
    let context_interface = module.add_interface(make_context_interface());
    let context_storage_interface = module.add_interface(make_context_storage_interface());
    let execution_context_aware_interface =
        module.add_interface(make_execution_context_aware_interface());
    let context_storage_scope_interface = module.add_interface(
        make_context_storage_scope_interface(scope_interface.clone()),
    );
    let _span_kind_interface = module.add_interface(make_span_kind_interface());
    let _trace_flags_interface = module.add_interface(make_trace_flags_interface());
    let trace_state_interface = module.add_interface(make_trace_state_interface());
    let span_context_interface = module.add_interface(make_span_context_interface());
    let span_interface = module.add_interface(make_span_interface(
        implicit_context_keyed_interface.clone(),
    ));
    let span_builder_interface = module.add_interface(make_span_builder_interface());
    let tracer_interface = module.add_interface(make_tracer_interface());
    let tracer_provider_interface = module.add_interface(make_tracer_provider_interface());
    let propagation_getter_interface = module.add_interface(make_propagation_getter_interface());
    let extended_propagation_getter_interface = module.add_interface(
        make_extended_propagation_getter_interface(propagation_getter_interface.clone()),
    );
    let propagation_setter_interface = module.add_interface(make_propagation_setter_interface());
    let text_map_propagator_interface = module.add_interface(make_text_map_propagator_interface());
    let response_propagator_interface = module.add_interface(make_response_propagator_interface());
    let _signals_interface = module.add_interface(make_signals_interface());
    // Metrics interfaces mirror open-telemetry/api 1.10 exactly. Register the
    // inheritance roots first so Zend can enforce implementation compatibility.
    let metrics_instrument_interface =
        module.add_interface(interfaces::make_instrument_interface());
    let synchronous_instrument_interface = module.add_interface(
        interfaces::make_synchronous_instrument_interface(metrics_instrument_interface.clone()),
    );
    let asynchronous_instrument_interface = module.add_interface(
        interfaces::make_asynchronous_instrument_interface(metrics_instrument_interface),
    );
    let metrics_observer_interface = module.add_interface(interfaces::make_observer_interface());
    let observable_callback_interface =
        module.add_interface(interfaces::make_observable_callback_interface());
    let counter_interface = module.add_interface(interfaces::make_counter_interface(
        synchronous_instrument_interface.clone(),
    ));
    let up_down_counter_interface = module.add_interface(
        interfaces::make_up_down_counter_interface(synchronous_instrument_interface.clone()),
    );
    let histogram_interface = module.add_interface(interfaces::make_histogram_interface(
        synchronous_instrument_interface.clone(),
    ));
    let gauge_interface = module.add_interface(interfaces::make_gauge_interface(
        synchronous_instrument_interface,
    ));
    let observable_counter_interface = module.add_interface(
        interfaces::make_observable_counter_interface(asynchronous_instrument_interface.clone()),
    );
    let observable_up_down_counter_interface =
        module.add_interface(interfaces::make_observable_up_down_counter_interface(
            asynchronous_instrument_interface.clone(),
        ));
    let observable_gauge_interface = module.add_interface(
        interfaces::make_observable_gauge_interface(asynchronous_instrument_interface),
    );
    let meter_interface = module.add_interface(interfaces::make_meter_interface());
    let meter_provider_interface =
        module.add_interface(interfaces::make_meter_provider_interface());

    // co-dependent classes
    let logger_interface = module.add_interface(make_logger_interface());
    let log_record_builder_interface = module.add_interface(make_log_record_builder_interface());
    let event_logger_interface = module.add_interface(make_event_logger_interface());
    let logger_provider_interface = module.add_interface(make_logger_provider_interface());
    let event_logger_provider_interface =
        module.add_interface(make_event_logger_provider_interface());
    let _severity_enum = module.add_enum(make_severity_enum());
    let context_key_class = module.add_class(make_context_key_class(context_key_interface));
    let context_keys_class = module.add_class(make_context_keys_class(context_key_class.clone()));
    let trace_state_class = module.add_class(make_trace_state_class(trace_state_interface));
    let mut scope_class_entity = new_scope_class();
    let mut context_class_entity = new_context_class();
    let mut storage_class_entity = new_storage_class();
    build_scope_class(
        &mut scope_class_entity,
        &context_class_entity,
        &context_storage_scope_interface,
    );
    build_context_class(
        &mut context_class_entity,
        &scope_class_entity,
        &storage_class_entity,
        context_key_class.clone(),
        context_interface,
    );
    build_storage_class(
        &mut storage_class_entity,
        &scope_class_entity,
        &context_class_entity,
        &context_storage_interface,
        &execution_context_aware_interface,
    );

    let array_access_getter_setter_class =
        module.add_class(make_array_access_getter_setter_class(
            extended_propagation_getter_interface,
            propagation_setter_interface,
        ));
    let multi_text_map_propagator_class = module.add_class(make_multi_text_map_propagator_class(
        text_map_propagator_interface.clone(),
        context_class_entity.bound_class(),
    ));
    let native_noop_response_propagator_class = module.add_class(
        make_native_noop_response_propagator_class(response_propagator_interface),
    );

    let span_context_class =
        module.add_class(make_span_context_class(span_context_interface));
    let _scope_class = module.add_class(scope_class_entity);
    let context_class = module.add_class(context_class_entity);
    let _storage_class = module.add_class(storage_class_entity);
    let metadata_class = module.add_class(make_metadata_class(metadata_interface));
    let entry_class = module.add_class(make_entry_class());
    let baggage_builder_class = module.add_class(make_baggage_builder_class(
        baggage_builder_interface,
        entry_class,
        metadata_class.clone(),
    ));
    let baggage_class = module.add_class(make_baggage_class(
        baggage_interface,
        context_class.clone(),
        context_key_class.clone(),
        context_keys_class.clone(),
    ));
    let _baggage_parser_class = module.add_class(make_parser_class(metadata_class.clone()));
    let _in_memory_exporter_class = module.add_class(make_memory_exporter_class());
    let _logs_memory_exporter_class = module.add_class(make_logs_memory_exporter_class());
    let _metrics_memory_exporter_class = module.add_class(make_metrics_memory_exporter_class());

    let trace_context_propagator_class = module.add_class(make_trace_context_propagator_class(
        text_map_propagator_interface.clone(),
        context_class.clone(),
        context_key_class.clone(),
        context_keys_class.clone(),
        span_context_class.clone(),
        trace_state_class.clone(),
        array_access_getter_setter_class.clone(),
    ));
    let baggage_propagator_class = module.add_class(make_baggage_propagator_class(
        text_map_propagator_interface,
        context_class.clone(),
        context_key_class.clone(),
        context_keys_class.clone(),
        baggage_class,
        baggage_builder_class,
        metadata_class,
        array_access_getter_setter_class,
    ));

    let observer_class = module.add_class(make_observer_class(metrics_observer_interface));
    let observable_callback_class = module.add_class(make_observable_callback_class(
        observable_callback_interface,
    ));
    let counter_class = module.add_class(make_counter_class(counter_interface));
    let up_down_counter_class =
        module.add_class(make_up_down_counter_class(up_down_counter_interface));
    let histogram_class = module.add_class(make_histogram_class(histogram_interface));
    let gauge_class = module.add_class(make_gauge_class(gauge_interface));
    let observable_counter_class = module.add_class(make_asynchronous_class(
        OBSERVABLE_COUNTER_CLASS_NAME,
        observable_counter_interface,
        observable_callback_class.clone(),
        observer_class.clone(),
    ));
    let observable_up_down_counter_class = module.add_class(make_asynchronous_class(
        OBSERVABLE_UP_DOWN_COUNTER_CLASS_NAME,
        observable_up_down_counter_interface,
        observable_callback_class.clone(),
        observer_class.clone(),
    ));
    let observable_gauge_class = module.add_class(make_asynchronous_class(
        OBSERVABLE_GAUGE_CLASS_NAME,
        observable_gauge_interface,
        observable_callback_class.clone(),
        observer_class.clone(),
    ));
    let meter_class = module.add_class(make_meter_class(
        meter_interface,
        counter_class,
        up_down_counter_class,
        histogram_class,
        gauge_class,
        observable_counter_class,
        observable_up_down_counter_class,
        observable_gauge_class,
        observer_class,
        observable_callback_class,
    ));
    let meter_provider_class = module.add_class(make_meter_provider_class(
        meter_provider_interface,
        meter_class,
    ));

    let span_base_class = module.add_class(make_span_base_class(
        context_class.clone(),
        context_key_class.clone(),
        context_keys_class.clone(),
        span_context_class.clone(),
        trace_state_class.clone(),
        span_interface,
    ));
    let span_class = module.add_class(make_span_class(
        span_base_class.clone(),
        span_context_class.clone(),
        trace_state_class.clone(),
    ));
    let non_recording_span_class = module.add_class(make_non_recording_span_class(
        span_base_class,
        span_context_class.clone(),
    ));
    let span_builder_class = module.add_class(make_span_builder_class(
        span_class.clone(),
        non_recording_span_class.clone(),
        context_class.clone(),
        context_key_class.clone(),
        context_keys_class.clone(),
        span_builder_interface,
    ));
    let _local_root_span_class = module.add_class(make_local_root_span_class(
        context_class.clone(),
        context_key_class.clone(),
        context_keys_class.clone(),
        span_class.clone(),
        non_recording_span_class.clone(),
    ));

    let tracer_class = module.add_class(make_tracer_class(
        span_builder_class.clone(),
        tracer_interface,
    ));
    let tracer_provider_class = module.add_class(make_tracer_provider_class(
        tracer_class.clone(),
        tracer_provider_interface,
    ));
    let _log_record_class = module.add_class(make_log_record_class(context_class.clone()));
    let log_record_builder_class = module.add_class(make_log_record_builder_class(
        log_record_builder_interface,
        context_class.clone(),
    ));
    let logger_class = module.add_class(make_logger_class(
        logger_interface,
        log_record_builder_class,
    ));
    let event_logger_class = module.add_class(make_event_logger_class(
        event_logger_interface,
        context_class.clone(),
    ));
    let logger_provider_class = module.add_class(make_logger_provider_class(
        logger_class.clone(),
        event_logger_class,
        logger_provider_interface.clone(),
        event_logger_provider_interface,
    ));
    let _globals_class = module.add_class(make_globals_class(
        tracer_provider_class.clone(),
        meter_provider_class.clone(),
        logger_provider_class.clone(),
        trace_context_propagator_class.clone(),
        baggage_propagator_class,
        multi_text_map_propagator_class,
        native_noop_response_propagator_class,
        context_class,
    ));
    let _status_code_interface = module.add_interface(make_status_code_interface());
}
