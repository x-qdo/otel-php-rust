--TEST--
A panic in RSHUTDOWN is contained: the process exits normally
--EXTENSIONS--
otel
--SKIPIF--
<?php if (!function_exists('otel_test_panic')) die('skip requires a --features test build'); ?>
--INI--
otel.cli.enabled=1
otel.log.level="off"
--ENV--
OTEL_TEST_PANIC_AT=rshutdown
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\SpanExporter\Memory;

$span = Globals::tracerProvider()->getTracer('panic-test')->spanBuilder('before-shutdown')->startSpan();
$span->end();
var_dump(Memory::count());
register_shutdown_function(function () {
    echo "shutdown function ran\n";
});
echo "done\n";
?>
--EXPECT--
int(1)
done
shutdown function ran
