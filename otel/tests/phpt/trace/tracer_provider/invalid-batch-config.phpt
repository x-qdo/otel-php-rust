--TEST--
Invalid bounded batch configuration degrades to a no-op provider
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
--ENV--
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4317
OTEL_EXPORTER_OTLP_PROTOCOL=grpc
OTEL_BSP_MAX_QUEUE_SIZE=0
OTEL_BSP_MAX_EXPORT_BATCH_SIZE=2
--FILE--
<?php
use OpenTelemetry\API\Globals;

$span = Globals::tracerProvider()
    ->getTracer('invalid-batch-config-test')
    ->spanBuilder('safe-no-op')
    ->startSpan();
var_dump($span->isRecording());
$span->end();
echo "alive\n";
?>
--EXPECT--
bool(false)
alive
