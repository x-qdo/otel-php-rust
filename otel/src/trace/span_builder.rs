use crate::{
    context::storage,
    trace::{non_recording_span::NonRecordingSpanClass, span::SpanClass},
};
use opentelemetry::{
    Context,
    trace::{Link, SpanBuilder, SpanContext, SpanKind, Tracer},
};
use opentelemetry_sdk::trace::SdkTracer;
use phper::{
    alloc::ToRefOwned,
    classes::{ClassEntity, StateClass, Visibility},
    functions::Argument,
    types::ArgumentTypeHint,
};
use std::{
    convert::Infallible,
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

pub struct SpanBuilderState {
    span_builder: Option<SpanBuilder>,
    tracer: Option<SdkTracer>,
    parent_context_id: u64,
}
// @see https://github.com/open-telemetry/opentelemetry-rust/issues/2742
impl SpanBuilderState {
    pub fn new(span_builder: SpanBuilder, tracer: SdkTracer) -> Self {
        Self {
            span_builder: Some(span_builder),
            tracer: Some(tracer),
            parent_context_id: 0,
        }
    }
    pub fn empty() -> Self {
        Self {
            span_builder: None,
            tracer: None,
            parent_context_id: 0,
        }
    }
}

const SPAN_BUILDER_CLASS_NAME: &str = r"OpenTelemetry\API\Trace\SpanBuilder";

pub type SpanBuilderClass = StateClass<SpanBuilderState>;

pub fn make_span_builder_class(
    span_class: SpanClass,
    non_recording_span_class: NonRecordingSpanClass,
) -> ClassEntity<SpanBuilderState> {
    let mut class = ClassEntity::<SpanBuilderState>::new_with_state_constructor(
        SPAN_BUILDER_CLASS_NAME,
        || SpanBuilderState::empty(),
    );

    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method("setAttribute", Visibility::Public, |this, arguments| {
            let name = crate::util::attribute_key(crate::util::arg(arguments, 0)?.expect_z_str()?.to_str()?);
            let Some(attribute) = crate::util::zval_to_key_value(name, crate::util::arg(arguments, 1)?) else {
                return Ok::<_, phper::Error>(this.to_ref_owned());
            };
            let Some(span_builder) = this.as_mut_state().span_builder.as_mut() else {
                return Ok::<_, phper::Error>(this.to_ref_owned());
            };
            span_builder
                .attributes
                .get_or_insert_with(Vec::new)
                .push(attribute);

            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("key"))
        .argument(Argument::new("value").optional());

    class
        .add_method("setAttributes", Visibility::Public, |this, arguments| {
            let attributes = crate::util::zval_arr_to_key_value_vec(crate::util::arg(arguments, 0)?.expect_z_arr()?);
            let Some(span_builder) = this.as_mut_state().span_builder.as_mut() else {
                return Ok::<_, phper::Error>(this.to_ref_owned());
            };
            match span_builder.attributes.as_mut() {
                Some(existing) => existing.extend(attributes),
                None => span_builder.attributes = Some(attributes),
            }

            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("attributes"));

    class
        .add_method(
            "setStartTimestamp",
            Visibility::Public,
            |this, arguments| {
                let timestamp_nanos = crate::util::arg(arguments, 0)?.expect_long()?;
                if timestamp_nanos < 0 {
                    tracing::warn!("SpanBuilder::setStartTimestamp ignored a negative timestamp");
                    return Ok::<_, phper::Error>(this.to_ref_owned());
                }
                let Some(span_builder) = this.as_mut_state().span_builder.as_mut() else {
                    return Ok::<_, phper::Error>(this.to_ref_owned());
                };
                span_builder.start_time =
                    Some(UNIX_EPOCH + Duration::from_nanos(timestamp_nanos as u64));

                Ok::<_, phper::Error>(this.to_ref_owned())
            },
        )
        .argument(Argument::new("timestampNanos").with_type_hint(ArgumentTypeHint::Int));

    class
        .add_method("addLink", Visibility::Public, |this, arguments| {
            let span_context = {
                let span_context_obj = crate::util::arg_mut(arguments, 0)?.expect_mut_z_obj()?;
                let state_obj = unsafe { span_context_obj.as_state_obj::<Option<SpanContext>>() };
                let Some(span_context) = state_obj.as_state().as_ref() else {
                    tracing::warn!("SpanBuilder::addLink ignored an invalid SpanContext");
                    return Ok::<_, phper::Error>(this.to_ref_owned());
                };
                span_context.clone()
            };
            let attributes = arguments
                .get(1)
                .and_then(|argument| argument.as_z_arr())
                .map(crate::util::zval_arr_to_key_value_vec)
                .unwrap_or_default();
            let Some(span_builder) = this.as_mut_state().span_builder.as_mut() else {
                return Ok::<_, phper::Error>(this.to_ref_owned());
            };
            span_builder
                .links
                .get_or_insert_with(Vec::new)
                .push(Link::new(span_context, attributes, 0));

            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("context"))
        .argument(Argument::new("attributes").optional());

    class
        .add_method("setParent", Visibility::Public, |this, arguments| {
            let state = this.as_mut_state();

            let context_obj = crate::util::arg_mut(arguments, 0)?.expect_mut_z_obj()?;
            let context_id = context_obj
                .get_property("context_id")
                .as_long()
                .unwrap_or(0);
            state.parent_context_id = context_id as u64;

            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(
            Argument::new("context").with_type_hint(ArgumentTypeHint::ClassEntry(String::from(
                r"OpenTelemetry\Context\ContextInterface",
            ))),
        );

    class
        .add_method("setSpanKind", Visibility::Public, |this, arguments| {
            let span_kind_int = crate::util::arg(arguments, 0)?.expect_long()?;
            let span_kind = match span_kind_int {
                0 => SpanKind::Internal,
                1 => SpanKind::Server,
                2 => SpanKind::Client,
                3 => SpanKind::Producer,
                4 => SpanKind::Consumer,
                _ => {
                    //log a warning
                    SpanKind::Internal
                }
            };
            let Some(span_builder) = this.as_mut_state().span_builder.as_mut() else {
                return Ok::<_, phper::Error>(this.to_ref_owned());
            };
            span_builder.span_kind = Some(span_kind);

            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("spanKind").with_type_hint(ArgumentTypeHint::Int));

    class.add_method("startSpan", Visibility::Public, move |this, _| {
        let state = this.as_state();
        let (Some(span_builder), Some(tracer)) =
            (state.span_builder.as_ref(), state.tracer.as_ref())
        else {
            // No-op provider: no span context is generated, mirroring the
            // official SDK's disabled behaviour.
            return Ok::<_, phper::Error>(non_recording_span_class.init_object()?);
        };
        let parent_context = if state.parent_context_id > 0 {
            storage::get_context_instance(Some(state.parent_context_id))
                .map(|ctx| {
                    tracing::debug!(
                        "SpanBuilder::Using parent context {} (ref count = {})",
                        state.parent_context_id,
                        Arc::strong_count(&ctx)
                    );
                    (*ctx).clone()
                })
                .unwrap_or_else(|| {
                    tracing::warn!(
                        "SpanBuilder::Parent context {} not found, falling back to current()",
                        state.parent_context_id
                    );
                    Context::current()
                })
        } else {
            tracing::debug!("SpanBuilder::No parent context, using Context::current()");
            Context::current()
        };

        let span = tracer.build_with_context(span_builder.clone(), &parent_context);
        tracing::debug!("SpanBuilder::Starting span");
        let mut object = span_class.init_object()?;
        *object.as_mut_state() = Some(span);
        Ok::<_, phper::Error>(object)
    });

    class
}
