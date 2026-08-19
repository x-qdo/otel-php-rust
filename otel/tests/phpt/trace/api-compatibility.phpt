--TEST--
Native Trace API supports official value semantics and userland ContextInterface implementations
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
otel.log.level=error
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\LocalRootSpan;
use OpenTelemetry\API\Trace\NonRecordingSpan;
use OpenTelemetry\API\Trace\Propagation\TraceContextPropagator;
use OpenTelemetry\API\Trace\Span;
use OpenTelemetry\API\Trace\SpanContext;
use OpenTelemetry\API\Trace\SpanExporter\Memory;
use OpenTelemetry\API\Trace\SpanKind;
use OpenTelemetry\API\Trace\TraceFlags;
use OpenTelemetry\API\Trace\TraceState;
use OpenTelemetry\Context\Context;
use OpenTelemetry\Context\ContextInterface;
use OpenTelemetry\Context\ContextKeyInterface;
use OpenTelemetry\Context\ImplicitContextKeyedInterface;
use OpenTelemetry\Context\ScopeInterface;

function check(string $name, bool $condition): void
{
    echo $name, ':', $condition ? 'ok' : 'fail', "\n";
}

final class UserContext implements ContextInterface
{
    private array $values = [];

    public static function createKey(string $key): ContextKeyInterface
    {
        return Context::createKey($key);
    }

    public static function getCurrent(): ContextInterface
    {
        return new self();
    }

    public function activate(): ScopeInterface
    {
        return Context::getRoot()->activate();
    }

    public function with(ContextKeyInterface $key, $value): ContextInterface
    {
        $next = clone $this;
        $next->values[spl_object_id($key)] = $value;
        return $next;
    }

    public function withContextValue(ImplicitContextKeyedInterface $value): ContextInterface
    {
        return $value->storeInContext($this);
    }

    public function get(ContextKeyInterface $key)
    {
        return $this->values[spl_object_id($key)] ?? null;
    }
}

check('constants',
    SpanKind::KIND_CLIENT === 1
    && SpanKind::KIND_SERVER === 2
    && TraceFlags::DEFAULT === 0
    && TraceFlags::SAMPLED === 1
    && TraceFlags::RANDOM === 2
);

$state = new TraceState('vendor=value,foo=bar');
$updated = $state->with('foo', 'new');
check('tracestate-parse', (string) $state === 'vendor=value,foo=bar' && $state->getListMemberCount() === 2);
check('tracestate-immutable', (string) $updated === 'foo=new,vendor=value' && $state->get('foo') === 'bar');
check('tracestate-noop', $state->without('missing') === $state && $state->with('Bad', 'value') === $state);
$large = new TraceState('large=' . str_repeat('x', 129) . ',small=value');
check('tracestate-limit', $large->toString(11) === 'small=value');

$traceId = '2b4ef3412d587ce6e7880fb27a316b8c';
$spanId = '7480a670201f6340';
$spanContext = SpanContext::create($traceId, $spanId, TraceFlags::SAMPLED | TraceFlags::RANDOM, $updated);
check('span-context',
    $spanContext->isValid()
    && !$spanContext->isRemote()
    && $spanContext->isSampled()
    && $spanContext->getTraceFlags() === 3
    && strlen($spanContext->getTraceIdBinary()) === 16
    && strlen($spanContext->getSpanIdBinary()) === 8
    && $spanContext->getTraceState() === $updated
);
$wideFlags = SpanContext::create($traceId, $spanId, 259);
check('span-context-flags', $wideFlags->getTraceFlags() === 259 && $wideFlags->isSampled());
check('invalid-singletons', SpanContext::getInvalid() === SpanContext::getInvalid() && Span::getInvalid() === Span::getInvalid());

$remote = SpanContext::createFromRemoteParent($traceId, $spanId, TraceFlags::SAMPLED, $state);
$wrapped = Span::wrap($remote);
check('span-wrap', $wrapped instanceof NonRecordingSpan && !$wrapped->isRecording() && $wrapped->getContext() === $remote);
check('clone-semantics',
    (clone $spanContext)->getTraceId() === $traceId
    && (string) (clone $state) === 'vendor=value,foo=bar'
    && (clone $wrapped)->getContext() === $remote
    && (clone TraceContextPropagator::getInstance())->fields() === ['traceparent', 'tracestate']
    && (clone new LocalRootSpan()) instanceof LocalRootSpan
);

$custom = $wrapped->storeInContext(new UserContext());
check('custom-context-store', $custom instanceof UserContext && Span::fromContext($custom) === $wrapped);
check('custom-local-root', LocalRootSpan::fromContext($custom) === $wrapped);

$propagator = TraceContextPropagator::getInstance();
check('propagator-singleton', $propagator === TraceContextPropagator::getInstance());
check('propagator-fields', $propagator->fields() === ['traceparent', 'tracestate']);
$carrier = [];
$propagator->inject($carrier, null, $custom);
check('custom-context-inject',
    $carrier['traceparent'] === '00-' . $traceId . '-' . $spanId . '-01'
    && $carrier['tracestate'] === 'vendor=value,foo=bar'
);
$extracted = $propagator->extract($carrier, null, new UserContext());
check('custom-context-extract',
    $extracted instanceof UserContext
    && Span::fromContext($extracted)->getContext()->isRemote()
    && Span::fromContext($extracted)->getContext()->getTraceId() === $traceId
);

$attributes = (function (): Generator {
    yield 'source' => 'generator';
})();
$tracer = Globals::tracerProvider()->getTracer('trace-compat', null, null, $attributes);
check('tracer-enabled', $tracer->isEnabled());
$child = $tracer->spanBuilder('custom-parent')->setParent($custom)->startSpan();
$child->addEvent('event-with-defaults')->addLink($remote)->end();
$root = $tracer->spanBuilder('forced-root')->setParent(false)->startSpan();
$root->end();

$spans = Memory::getSpans();
check('custom-context-parent',
    $spans[0]['span_context']['trace_id'] === $traceId
    && $spans[0]['parent_span_id'] === $spanId
    && $spans[0]['instrumentation_scope']['attributes']['source'] === 'generator'
);
check('false-parent', $spans[1]['parent_span_id'] === '0000000000000000');
?>
--EXPECT--
constants:ok
tracestate-parse:ok
tracestate-immutable:ok
tracestate-noop:ok
tracestate-limit:ok
span-context:ok
span-context-flags:ok
invalid-singletons:ok
span-wrap:ok
clone-semantics:ok
custom-context-store:ok
custom-local-root:ok
propagator-singleton:ok
propagator-fields:ok
custom-context-inject:ok
custom-context-extract:ok
tracer-enabled:ok
custom-context-parent:ok
false-parent:ok
