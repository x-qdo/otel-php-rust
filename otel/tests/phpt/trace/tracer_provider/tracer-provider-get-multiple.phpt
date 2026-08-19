--TEST--
Get multiple tracer providers from Globals
--EXTENSIONS--
otel
--FILE--
<?php
use OpenTelemetry\API\Globals;

$one = Globals::tracerProvider();
$two = Globals::tracerProvider();
var_dump($one === $two);
?>
--EXPECT--
bool(true)
