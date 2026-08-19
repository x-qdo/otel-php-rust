use crate::{
    context::context_class::ContextClass,
    error::php_exception_to_attributes,
    logs::log_record::{
        LogRecordState, any_value, nanos_to_system_time, set_attribute, set_attributes,
        set_context, set_otel_attribute, set_severity,
    },
};
use opentelemetry_sdk::logs::SdkLogger;
use phper::{
    alloc::ToRefOwned,
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};
use std::convert::Infallible;

pub const LOG_RECORD_BUILDER_CLASS_NAME: &str = r"OpenTelemetry\API\Logs\LogRecordBuilder";
const BUILDER_INTERFACE: &str = r"OpenTelemetry\API\Logs\LogRecordBuilderInterface";
const CONTEXT_INTERFACE: &str = r"OpenTelemetry\Context\ContextInterface";
const SEVERITY_ENUM: &str = r"OpenTelemetry\API\Logs\Severity";

#[derive(Clone, Default)]
pub struct LogRecordBuilderState {
    pub logger: Option<SdkLogger>,
    pub enabled: bool,
    pub record: LogRecordState,
}

pub type LogRecordBuilderClass = StateClass<LogRecordBuilderState>;

fn builder_return() -> ReturnType {
    ReturnType::new(ReturnTypeHint::ClassEntry(BUILDER_INTERFACE.to_string()))
}

pub fn make_log_record_builder_class(
    interface: Interface,
    context_class: ContextClass,
) -> ClassEntity<LogRecordBuilderState> {
    let mut class: ClassEntity<LogRecordBuilderState> =
        ClassEntity::new_with_default_state_constructor(LOG_RECORD_BUILDER_CLASS_NAME);
    class.set_final();
    class.implements(interface);
    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method("setTimestamp", Visibility::Public, |this, arguments| {
            let nanos = crate::util::arg(arguments, 0)?.expect_long()?;
            this.as_mut_state().record.timestamp = Some(nanos_to_system_time(nanos));
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("timestamp").with_type_hint(ArgumentTypeHint::Int))
        .return_type(builder_return());

    class
        .add_method(
            "setObservedTimestamp",
            Visibility::Public,
            |this, arguments| {
                let nanos = crate::util::arg(arguments, 0)?.expect_long()?;
                this.as_mut_state().record.observed_timestamp = Some(nanos_to_system_time(nanos));
                Ok::<_, phper::Error>(this.to_ref_owned())
            },
        )
        .argument(Argument::new("timestamp").with_type_hint(ArgumentTypeHint::Int))
        .return_type(builder_return());

    class
        .add_method("setContext", Visibility::Public, move |this, arguments| {
            set_context(
                &mut this.as_mut_state().record,
                crate::util::arg(arguments, 0)?,
                &context_class,
            )?;
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(
            Argument::new("context")
                .with_type_hint(ArgumentTypeHint::Union(vec![
                    ArgumentTypeHint::ClassEntry(CONTEXT_INTERFACE.to_string()),
                    ArgumentTypeHint::False,
                ]))
                .allow_null(),
        )
        .return_type(builder_return());

    class
        .add_method(
            "setSeverityNumber",
            Visibility::Public,
            |this, arguments| {
                set_severity(
                    &mut this.as_mut_state().record,
                    crate::util::arg(arguments, 0)?,
                )?;
                Ok::<_, phper::Error>(this.to_ref_owned())
            },
        )
        .argument(
            Argument::new("severityNumber").with_type_hint(ArgumentTypeHint::Union(vec![
                ArgumentTypeHint::ClassEntry(SEVERITY_ENUM.to_string()),
                ArgumentTypeHint::Int,
            ])),
        )
        .return_type(builder_return());

    class
        .add_method("setSeverityText", Visibility::Public, |this, arguments| {
            this.as_mut_state().record.severity_text = Some(
                crate::util::arg(arguments, 0)?
                    .expect_z_str()?
                    .to_str()?
                    .to_string(),
            );
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("severityText").with_type_hint(ArgumentTypeHint::String))
        .return_type(builder_return());

    class
        .add_method("setBody", Visibility::Public, |this, arguments| {
            this.as_mut_state().record.body = any_value(crate::util::arg(arguments, 0)?);
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("body").with_type_hint(ArgumentTypeHint::Mixed))
        .return_type(builder_return());

    class
        .add_method("setAttribute", Visibility::Public, |this, arguments| {
            let key = crate::util::arg(arguments, 0)?
                .expect_z_str()?
                .to_str()?
                .to_string();
            set_attribute(
                &mut this.as_mut_state().record,
                key,
                crate::util::arg(arguments, 1)?,
            );
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .argument(Argument::new("value").with_type_hint(ArgumentTypeHint::Mixed))
        .return_type(builder_return());

    class
        .add_method("setAttributes", Visibility::Public, |this, arguments| {
            set_attributes(
                &mut this.as_mut_state().record,
                crate::util::arg(arguments, 0)?,
            )?;
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("attributes").with_type_hint(ArgumentTypeHint::Iterable))
        .return_type(builder_return());

    class
        .add_method("setException", Visibility::Public, |this, arguments| {
            let exception = crate::util::arg_mut(arguments, 0)?.expect_mut_z_obj()?;
            for attribute in crate::util::limit_key_values(
                php_exception_to_attributes(exception),
                crate::util::AttributeDestination::Log,
            ) {
                set_otel_attribute(&mut this.as_mut_state().record, attribute);
            }
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(
            Argument::new("exception")
                .with_type_hint(ArgumentTypeHint::ClassEntry("Throwable".to_string())),
        )
        .return_type(builder_return());

    class
        .add_method("setEventName", Visibility::Public, |this, arguments| {
            this.as_mut_state().record.event_name = Some(
                crate::util::arg(arguments, 0)?
                    .expect_z_str()?
                    .to_str()?
                    .to_string(),
            );
            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("eventName").with_type_hint(ArgumentTypeHint::String))
        .return_type(builder_return());

    class
        .add_method("emit", Visibility::Public, |this, _| {
            let state = this.as_state();
            if state.enabled
                && let Some(logger) = &state.logger
            {
                crate::logs::logger::emit_state(logger, &state.record);
            }
            Ok::<_, Infallible>(())
        })
        .return_type(ReturnType::new(ReturnTypeHint::Void));

    class
}
