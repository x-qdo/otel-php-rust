--TEST--
Trace ID ratio sampler makes deterministic decisions for fixed trace IDs
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
OTEL_TRACES_SAMPLER=traceidratio
OTEL_TRACES_SAMPLER_ARG=0.01
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\SpanExporter\Memory;

$tracer = Globals::tracerProvider()->getTracer('sampler-test');
$sampledParent = Globals::propagator()->extract([
    'traceparent' => '00-11111111111111110000000000000001-d6ba64af4fa59b65-00',
]);
$droppedParent = Globals::propagator()->extract([
    'traceparent' => '00-1111111111111111ffffffffffffffff-d6ba64af4fa59b65-00',
]);

$sampled = $tracer->spanBuilder('sampled')->setParent($sampledParent)->startSpan();
$dropped = $tracer->spanBuilder('dropped')->setParent($droppedParent)->startSpan();
var_dump($sampled->isRecording());
var_dump($dropped->isRecording());
$sampled->end();
$dropped->end();
var_dump(Memory::count());
var_dump(Memory::getSpans()[0]['span_context']['trace_id']);
?>
--EXPECT--
bool(true)
bool(false)
int(1)
string(32) "11111111111111110000000000000001"
