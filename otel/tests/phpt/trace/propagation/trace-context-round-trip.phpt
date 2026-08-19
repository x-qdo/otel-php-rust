--TEST--
W3C trace context extraction and default-current injection preserve trace state
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

$propagator = Globals::propagator();
$parent = $propagator->extract([
    'traceparent' => '00-e77388f01a826e2de7afdcd1eefc034e-d6ba64af4fa59b65-01',
    'tracestate' => 'vendor=value',
]);
$span = Globals::tracerProvider()
    ->getTracer('propagation-test')
    ->spanBuilder('remote-child')
    ->setParent($parent)
    ->startSpan();
$spanId = $span->getContext()->getSpanId();
$scope = $span->activate();

$carrier = [];
$propagator->inject($carrier);
[$version, $traceId, $injectedSpanId, $flags] = explode('-', $carrier['traceparent']);
var_dump($version);
var_dump($traceId);
var_dump($injectedSpanId === $spanId);
var_dump($flags);
var_dump($carrier['tracestate']);

$scope->detach();
$span->end();
?>
--EXPECT--
string(2) "00"
string(32) "e77388f01a826e2de7afdcd1eefc034e"
bool(true)
string(2) "01"
string(12) "vendor=value"
