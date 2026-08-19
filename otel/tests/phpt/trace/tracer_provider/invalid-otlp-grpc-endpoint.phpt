--TEST--
Invalid OTLP gRPC endpoint degrades to a no-op provider without crossing FFI
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
--ENV--
OTEL_EXPORTER_OTLP_PROTOCOL=grpc
OTEL_EXPORTER_OTLP_ENDPOINT=not-a-url
--FILE--
<?php
use OpenTelemetry\API\Globals;

$span = Globals::tracerProvider()
    ->getTracer('invalid-config-test')
    ->spanBuilder('safe-no-op')
    ->startSpan();
var_dump($span->isRecording());
$span->end();
echo "alive\n";
?>
--EXPECT--
bool(false)
alive
