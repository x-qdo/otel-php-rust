--TEST--
SpanBuilder preserves explicit start time and link attributes
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
use OpenTelemetry\API\Trace\SpanContext;
use OpenTelemetry\API\Trace\SpanExporter\Memory;

$link = SpanContext::create(
    '2b4ef3412d587ce6e7880fb27a316b8c',
    '7480a670201f6340',
);
$span = Globals::tracerProvider()
    ->getTracer('builder-contract-test')
    ->spanBuilder('root')
    ->setStartTimestamp(1700000000123456789)
    ->addLink($link, ['link.type' => 'batch', 'link.attempt' => 2])
    ->startSpan();
$span->end();

$exported = Memory::getSpans()[0];
var_dump($exported['start_time']);
var_dump($exported['links']);
?>
--EXPECT--
int(1700000000123456)
array(1) {
  [0]=>
  array(2) {
    ["span_context"]=>
    array(4) {
      ["trace_id"]=>
      string(32) "2b4ef3412d587ce6e7880fb27a316b8c"
      ["span_id"]=>
      string(16) "7480a670201f6340"
      ["trace_flags"]=>
      string(2) "01"
      ["is_remote"]=>
      bool(false)
    }
    ["attributes"]=>
    array(2) {
      ["link.type"]=>
      string(5) "batch"
      ["link.attempt"]=>
      int(2)
    }
  }
}
