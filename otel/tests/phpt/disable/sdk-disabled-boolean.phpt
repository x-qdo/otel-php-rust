--TEST--
OTEL_SDK_DISABLED follows the OpenTelemetry boolean contract
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
OTEL_SDK_DISABLED=TRUE
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\SpanExporter\Memory;

$span = Globals::tracerProvider()
    ->getTracer('disabled-test')
    ->spanBuilder('must-not-record')
    ->startSpan();
var_dump($span->isRecording());
$span->end();
var_dump(Memory::count());
?>
--EXPECT--
bool(false)
int(0)
