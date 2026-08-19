--TEST--
Userland subclasses of native classes create state objects and dispatch inherited typed methods
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
--FILE--
<?php
use OpenTelemetry\API\Trace\NonRecordingSpan;
use OpenTelemetry\API\Trace\SpanExporter\Memory;
use OpenTelemetry\API\Trace\TracerProvider;
use OpenTelemetry\Context\Context;

// PHP copies internal functions into the arena for userland subclasses; the
// inherited typed methods (isRecording(): bool, getTracer(string), ...) must still
// reach the native handlers, and object creation must resolve the native state.
class SubSpan extends NonRecordingSpan {
    public function extra(): string { return 'sub'; }
}
class SubProvider extends TracerProvider {}

$span = (new ReflectionClass(SubSpan::class))->newInstanceWithoutConstructor();
var_dump($span->isRecording(), $span->extra(), $span->getContext()->isValid());

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
string(28) "OpenTelemetry\API\Trace\Span"
string(29) "OpenTelemetry\Context\Context"
int(2)
string(4) "root"
int(3)
string(7) "renamed"
bool(true)
