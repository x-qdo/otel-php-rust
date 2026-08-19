use crate::{
    context::{
        context_class::{ContextClass, current_context_value},
        context_key::{ContextKeyClass, ContextKeysClass},
    },
    trace::{
        non_recording_span::NonRecordingSpanClass,
        span::{SpanClass, otel_context_from_php},
        span_builder_interface::SPAN_BUILDER_INTERFACE,
        span_context::span_context_from_object,
        span_context_interface::SPAN_CONTEXT_INTERFACE,
        span_interface::SPAN_INTERFACE,
    },
    util::AttributeDestination,
};
use opentelemetry::{
    Context,
    trace::{Link, SpanBuilder, SpanKind, Tracer},
};
use opentelemetry_sdk::trace::SdkTracer;
use phper::{
    alloc::ToRefOwned,
    classes::{ClassEntity, Interface, StateClass, Visibility},
    functions::{Argument, ReturnType},
    types::{ArgumentTypeHint, ReturnTypeHint},
};
use std::{convert::Infallible, time::{Duration, UNIX_EPOCH}};

pub struct SpanBuilderState {
    span_builder: Option<SpanBuilder>,
    tracer: Option<SdkTracer>,
    parent: ParentContext,
}

enum ParentContext {
    Current,
    Root,
    Explicit(Context),
}
// @see https://github.com/open-telemetry/opentelemetry-rust/issues/2742
impl SpanBuilderState {
    pub fn new(span_builder: SpanBuilder, tracer: SdkTracer) -> Self {
        Self {
            span_builder: Some(span_builder),
            tracer: Some(tracer),
            parent: ParentContext::Current,
        }
    }
    pub fn empty() -> Self {
        Self {
            span_builder: None,
            tracer: None,
            parent: ParentContext::Current,
        }
    }
}

const SPAN_BUILDER_CLASS_NAME: &str = r"OpenTelemetry\API\Trace\SpanBuilder";

pub type SpanBuilderClass = StateClass<SpanBuilderState>;

pub fn make_span_builder_class(
    span_class: SpanClass,
    non_recording_span_class: NonRecordingSpanClass,
    context_class: ContextClass,
    key_class: ContextKeyClass,
    keys_class: ContextKeysClass,
    interface: Interface,
) -> ClassEntity<SpanBuilderState> {
    let mut class = ClassEntity::<SpanBuilderState>::new_with_state_constructor(
        SPAN_BUILDER_CLASS_NAME,
        SpanBuilderState::empty,
    );
    class.implements(interface);

    class.add_method("__construct", Visibility::Private, |_, _| {
        Ok::<_, Infallible>(())
    });

    class
        .add_method("setAttribute", Visibility::Public, |this, arguments| {
            let name = crate::util::attribute_key(
                crate::util::arg(arguments, 0)?.expect_z_str()?.to_str()?,
            );
            let Some(attribute) = crate::util::zval_to_key_value(
                AttributeDestination::Span,
                name,
                crate::util::arg(arguments, 1)?,
            )
            else {
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
        .argument(Argument::new("key").with_type_hint(ArgumentTypeHint::String))
        .argument(Argument::new("value").with_type_hint(ArgumentTypeHint::Mixed))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_BUILDER_INTERFACE.to_string(),
        )));

    class
        .add_method("setAttributes", Visibility::Public, |this, arguments| {
            let attributes = crate::util::zval_iterable_to_array(
                crate::util::arg(arguments, 0)?,
            )?;
            let attributes = crate::util::zval_arr_to_key_value_vec(
                attributes.expect_z_arr()?,
                AttributeDestination::Span,
            );
            let Some(span_builder) = this.as_mut_state().span_builder.as_mut() else {
                return Ok::<_, phper::Error>(this.to_ref_owned());
            };
            match span_builder.attributes.as_mut() {
                Some(existing) => existing.extend(attributes),
                None => span_builder.attributes = Some(attributes),
            }

            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(Argument::new("attributes").with_type_hint(ArgumentTypeHint::Iterable))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_BUILDER_INTERFACE.to_string(),
        )));

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
        .argument(Argument::new("timestampNanos").with_type_hint(ArgumentTypeHint::Int))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_BUILDER_INTERFACE.to_string(),
        )));

    class
        .add_method("addLink", Visibility::Public, |this, arguments| {
            let span_context = {
                let span_context_obj = crate::util::arg_mut(arguments, 0)?.expect_mut_z_obj()?;
                span_context_from_object(span_context_obj)?
            };
            let attributes = if let Some(attributes) = arguments.get(1) {
                let attributes = crate::util::zval_iterable_to_array(attributes)?;
                crate::util::zval_arr_to_key_value_vec(
                    attributes.expect_z_arr()?,
                    AttributeDestination::Link,
                )
            } else {
                Vec::new()
            };
            let Some(span_builder) = this.as_mut_state().span_builder.as_mut() else {
                return Ok::<_, phper::Error>(this.to_ref_owned());
            };
            span_builder
                .links
                .get_or_insert_with(Vec::new)
                .push(Link::new(span_context, attributes, 0));

            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(
            Argument::new("context").with_type_hint(ArgumentTypeHint::ClassEntry(
                SPAN_CONTEXT_INTERFACE.to_string(),
            )),
        )
        .argument(
            Argument::new("attributes")
                .with_type_hint(ArgumentTypeHint::Iterable)
                .with_default_value("[]"),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_BUILDER_INTERFACE.to_string(),
        )));

    let parent_key_class = key_class.clone();
    let parent_keys_class = keys_class.clone();
    class
        .add_method("setParent", Visibility::Public, move |this, arguments| {
            let state = this.as_mut_state();
            let context = crate::util::arg_mut(arguments, 0)?;
            state.parent = if context.get_type_info().is_false() {
                ParentContext::Root
            } else if context.get_type_info().is_null() {
                ParentContext::Current
            } else {
                ParentContext::Explicit(otel_context_from_php(
                    context.expect_mut_z_obj()?,
                    &parent_key_class,
                    &parent_keys_class,
                )?)
            };

            Ok::<_, phper::Error>(this.to_ref_owned())
        })
        .argument(
            Argument::new("context").with_type_hint(ArgumentTypeHint::Union(vec![
                ArgumentTypeHint::ClassEntry(
                    r"OpenTelemetry\Context\ContextInterface".to_string(),
                ),
                ArgumentTypeHint::False,
                ArgumentTypeHint::Null,
            ])),
        )
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_BUILDER_INTERFACE.to_string(),
        )));

    class
        .add_method("setSpanKind", Visibility::Public, |this, arguments| {
            let span_kind_int = crate::util::arg(arguments, 0)?.expect_long()?;
            let span_kind = match span_kind_int {
                0 => SpanKind::Internal,
                1 => SpanKind::Client,
                2 => SpanKind::Server,
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
        .argument(Argument::new("spanKind").with_type_hint(ArgumentTypeHint::Int))
        .return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
            SPAN_BUILDER_INTERFACE.to_string(),
        )));

    class.add_method("startSpan", Visibility::Public, move |this, _| {
        let state = this.as_state();
        let (Some(span_builder), Some(tracer)) =
            (state.span_builder.as_ref(), state.tracer.as_ref())
        else {
            // No-op provider: no span context is generated, mirroring the
            // official SDK's disabled behaviour.
            return Ok::<_, phper::Error>(phper::values::ZVal::from(
                non_recording_span_class.init_object()?,
            ));
        };
        let parent_context = match &state.parent {
            ParentContext::Current => {
                let mut context = current_context_value(&context_class)?;
                otel_context_from_php(
                    context.expect_mut_z_obj()?,
                    &key_class,
                    &keys_class,
                )?
            }
            ParentContext::Root => Context::new(),
            ParentContext::Explicit(context) => context.clone(),
        };

        let span = tracer.build_with_context(span_builder.clone(), &parent_context);
        tracing::debug!("SpanBuilder::Starting span");
        let mut object = span_class.init_object()?;
        *object.as_mut_state() = Some(span);
        Ok::<_, phper::Error>(phper::values::ZVal::from(object))
    }).return_type(ReturnType::new(ReturnTypeHint::ClassEntry(
        SPAN_INTERFACE.to_string(),
    )));

    class
}
