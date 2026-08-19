--TEST--
Userland Span implementations and native provider subclasses dispatch inherited typed methods
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
--FILE--
<?php
use OpenTelemetry\API\Trace\Span;
use OpenTelemetry\API\Trace\SpanContext;
use OpenTelemetry\API\Trace\SpanContextInterface;
use OpenTelemetry\API\Trace\SpanExporter\Memory;
use OpenTelemetry\API\Trace\SpanInterface;
use OpenTelemetry\API\Trace\TracerProvider;
use OpenTelemetry\Context\Context;

// PHP copies internal functions into the arena for userland subclasses; the
// inherited typed methods (storeInContext(), getTracer(string), ...) must still
// reach the native handlers, and object creation must resolve native state.
class SubSpan extends Span {
    public function getContext(): SpanContextInterface { return SpanContext::getInvalid(); }
    public function isRecording(): bool { return false; }
    public function setAttribute(string $key, bool|int|float|string|array|null $value): SpanInterface { return $this; }
    public function setAttributes(iterable $attributes): SpanInterface { return $this; }
    public function addLink(SpanContextInterface $context, iterable $attributes = []): SpanInterface { return $this; }
    public function addEvent(string $name, iterable $attributes = [], ?int $timestamp = null): SpanInterface { return $this; }
    public function recordException(Throwable $exception, iterable $attributes = []): SpanInterface { return $this; }
    public function updateName(string $name): SpanInterface { return $this; }
    public function setStatus(string $code, ?string $description = null): SpanInterface { return $this; }
    public function end(?int $endEpochNanos = null): void {}
    public function extra(): string { return 'sub'; }
}
class SubProvider extends TracerProvider {}

$span = new SubSpan();
var_dump($span->isRecording(), $span->extra(), $span->getContext()->isValid());
var_dump(Span::fromContext($span->storeInContext(Context::getRoot())) === $span);

$provider = (new ReflectionClass(SubProvider::class))->newInstanceWithoutConstructor();
$tracer = $provider->getTracer('sub-tracer', '1.0');
$root = $tracer->spanBuilder('root')
    ->setAttribute('int', 1)
    ->setAttributes(['str' => 's', 'list' => [1, 2]])
    ->setSpanKind(2)
    ->startSpan();
var_dump($root->isRecording(), get_class($root));
$scope = $root->activate();
$child = $tracer->spanBuilder('child')->startSpan();
$child->setAttribute('z', 'y')->setStatus('Ok')->updateName('renamed')->addEvent('e', ['k' => 'v'])->end();
$scope->detach();
$root->end();
var_dump(get_class(Context::getCurrent()));
var_dump(Memory::count());
$spans = Memory::getSpans();
var_dump($spans[1]['name'], count($spans[1]['attributes']), $spans[0]['name'], $spans[0]['parent_span_id'] === $spans[1]['span_context']['span_id']);
?>
--EXPECT--
bool(false)
string(3) "sub"
bool(false)
bool(true)
bool(true)
string(34) "OpenTelemetry\API\Trace\NativeSpan"
string(29) "OpenTelemetry\Context\Context"
int(2)
string(4) "root"
int(3)
string(7) "renamed"
bool(true)
