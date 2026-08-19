--TEST--
Repeated span activation and cleanup never panic across the PHP FFI boundary
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
use OpenTelemetry\Context\ScopeInterface;

$span = Globals::tracerProvider()
    ->getTracer('repeated-lifecycle-test')
    ->spanBuilder('root')
    ->startSpan();

$first = $span->activate();
var_dump($first->detach());
$second = $span->activate();
var_dump($second instanceof ScopeInterface);
var_dump(is_int($second->detach()));
$span->end();
$span->end();
echo "alive\n";
?>
--EXPECT--
int(0)
bool(true)
bool(true)
alive
