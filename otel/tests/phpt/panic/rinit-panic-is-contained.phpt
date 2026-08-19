--TEST--
A panic in RINIT is contained: the request runs and the span path still works
--EXTENSIONS--
otel
--SKIPIF--
<?php if (!function_exists('otel_test_panic')) die('skip requires a --features test build'); ?>
--INI--
otel.cli.enabled=1
otel.log.level="off"
--ENV--
OTEL_TEST_PANIC_AT=rinit
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\SpanExporter\Memory;

echo "script runs\n";
$span = Globals::tracerProvider()->getTracer('panic-test')->spanBuilder('after-panic')->startSpan();
$span->end();
var_dump(Memory::count());
echo "done\n";
?>
--EXPECT--
script runs
int(1)
done
