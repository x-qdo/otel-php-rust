--TEST--
SpanBuilder preserves scalar and array attribute types
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
use OpenTelemetry\API\Trace\SpanExporter\Memory;

$span = Globals::tracerProvider()
    ->getTracer('builder-attribute-test')
    ->spanBuilder('root')
    ->setAttribute('enabled', true)
    ->setAttribute('attempt', 3)
    ->setAttributes([
        'ratio' => 1.5,
        'labels' => ['alpha', 'beta'],
        'counts' => [1, 2, 3],
    ])
    ->startSpan();
$span->end();

var_dump(Memory::getSpans()[0]['attributes']);
?>
--EXPECT--
array(5) {
  ["enabled"]=>
  bool(true)
  ["attempt"]=>
  int(3)
  ["ratio"]=>
  float(1.5)
  ["labels"]=>
  array(2) {
    [0]=>
    string(5) "alpha"
    [1]=>
    string(4) "beta"
  }
  ["counts"]=>
  array(3) {
    [0]=>
    int(1)
    [1]=>
    int(2)
    [2]=>
    int(3)
  }
}
