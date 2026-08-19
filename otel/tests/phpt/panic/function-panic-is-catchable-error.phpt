--TEST--
A panic inside a native function is a catchable \Error; the process and the span path continue
--EXTENSIONS--
otel
--SKIPIF--
<?php if (!function_exists('otel_test_panic')) die('skip requires a --features test build'); ?>
--INI--
otel.cli.enabled=1
otel.log.level="off"
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\SpanExporter\Memory;

try {
    otel_test_panic('function');
    echo "not reached\n";
} catch (\Error $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
echo "after catch\n";

$span = Globals::tracerProvider()->getTracer('panic-test')->spanBuilder('after-panic')->startSpan();
$span->setAttribute('k', 'v');
$span->end();
var_dump(Memory::count());
var_dump(Memory::getSpans()[0]['name']);
echo "done\n";
?>
--EXPECT--
Error: otel: internal error: test panic in function
after catch
int(1)
string(11) "after-panic"
done
