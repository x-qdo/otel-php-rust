--TEST--
A panic inside an auto-instrumentation observer hook is contained and the observed function still runs
--EXTENSIONS--
otel
--SKIPIF--
<?php if (!function_exists('otel_test_panic')) die('skip requires a --features test build'); ?>
--INI--
otel.cli.enabled=1
otel.auto.enabled=1
otel.log.level="off"
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\SpanExporter\Memory;

function otel_test_panic_hook_target(): string
{
    echo "target ran\n";
    return "target result";
}

for ($i = 0; $i < 2; $i++) {
    var_dump(otel_test_panic_hook_target());
}

$span = Globals::tracerProvider()->getTracer('panic-test')->spanBuilder('after-panic')->startSpan();
$span->end();
var_dump(Memory::count());
echo "done\n";
?>
--EXPECT--
target ran
string(13) "target result"
target ran
string(13) "target result"
int(1)
done
