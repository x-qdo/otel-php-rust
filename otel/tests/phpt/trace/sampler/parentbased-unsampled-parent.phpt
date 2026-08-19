--TEST--
Parent-based sampler preserves an unsampled remote parent decision
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
OTEL_TRACES_SAMPLER=parentbased_always_on
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\SpanExporter\Memory;

$parent = Globals::propagator()->extract([
    'traceparent' => '00-e77388f01a826e2de7afdcd1eefc034e-d6ba64af4fa59b65-00',
]);
$span = Globals::tracerProvider()
    ->getTracer('sampler-test')
    ->spanBuilder('unsampled-child')
    ->setParent($parent)
    ->startSpan();

var_dump($span->isRecording());
$span->end();
var_dump(Memory::count());
?>
--EXPECT--
bool(false)
int(0)
