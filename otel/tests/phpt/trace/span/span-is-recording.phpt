--TEST--
Recording span reports its lifecycle through isRecording
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
--FILE--
<?php
use OpenTelemetry\API\Globals;

$span = Globals::tracerProvider()
    ->getTracer('recording-test')
    ->spanBuilder('root')
    ->startSpan();

var_dump($span->isRecording());
$span->end();
var_dump($span->isRecording());
?>
--EXPECT--
bool(true)
bool(false)
