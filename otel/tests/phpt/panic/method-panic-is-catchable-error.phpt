--TEST--
A panic inside a native method or static method is a catchable \Error and the object stays usable
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
use OpenTelemetry\Test\PanicProbe;

$probe = new PanicProbe();
for ($i = 0; $i < 3; $i++) {
    try {
        $probe->panic();
        echo "not reached\n";
    } catch (\Error $e) {
        echo get_class($e), ": ", $e->getMessage(), "\n";
    }
}
try {
    PanicProbe::panicStatic();
    echo "not reached\n";
} catch (\Error $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
try {
    otel_test_panic('non-string');
} catch (\Error $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
var_dump($probe instanceof PanicProbe);

$span = Globals::tracerProvider()->getTracer('panic-test')->spanBuilder('after-panic')->startSpan();
$scope = $span->activate();
var_dump($scope->detach());
$span->end();
var_dump(Memory::count());
echo "done\n";
?>
--EXPECT--
Error: otel: internal error: test panic in method
Error: otel: internal error: test panic in method
Error: otel: internal error: test panic in method
Error: otel: internal error: test panic in static method
Error: otel: internal error: non-string panic payload
bool(true)
int(0)
int(1)
done
