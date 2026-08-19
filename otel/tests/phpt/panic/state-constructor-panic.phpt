--TEST--
A panic inside a native object state constructor is a catchable \Error on object creation
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
use OpenTelemetry\Test\PanicState;

try {
    $object = new PanicState();
    echo "not reached\n";
} catch (\Error $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
try {
    $object = (new ReflectionClass(PanicState::class))->newInstanceWithoutConstructor();
    echo "not reached\n";
} catch (\Error $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
echo "after catch\n";

$span = Globals::tracerProvider()->getTracer('panic-test')->spanBuilder('after-panic')->startSpan();
$span->end();
var_dump(Memory::count());
echo "done\n";
?>
--EXPECT--
Error: otel: internal error: test panic in state constructor
Error: otel: internal error: test panic in state constructor
after catch
int(1)
done
