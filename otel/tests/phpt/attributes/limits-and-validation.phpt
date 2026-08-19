--TEST--
Attribute limits and whole-array validation apply across traces, metrics, logs, and scopes
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
OTEL_METRICS_EXPORTER=memory
OTEL_LOGS_EXPORTER=memory
OTEL_LOGS_PROCESSOR=simple
OTEL_ATTRIBUTE_COUNT_LIMIT=2
OTEL_ATTRIBUTE_VALUE_LENGTH_LIMIT=4
OTEL_SPAN_ATTRIBUTE_COUNT_LIMIT=8
OTEL_SPAN_ATTRIBUTE_VALUE_LENGTH_LIMIT=3
OTEL_EVENT_ATTRIBUTE_COUNT_LIMIT=2
OTEL_LINK_ATTRIBUTE_COUNT_LIMIT=2
OTEL_LOGRECORD_ATTRIBUTE_COUNT_LIMIT=2
OTEL_SPAN_EVENT_COUNT_LIMIT=2
OTEL_SPAN_LINK_COUNT_LIMIT=1
OTEL_PHP_ATTRIBUTE_KEY_LENGTH_LIMIT=8
OTEL_PHP_ATTRIBUTE_ARRAY_LENGTH_LIMIT=2
--INI--
otel.cli.enabled=1
otel.auto.enabled=0
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Logs\LogRecord;
use OpenTelemetry\API\Logs\MemoryLogsExporter;
use OpenTelemetry\API\Metrics\MemoryMetricsExporter;
use OpenTelemetry\API\Trace\SpanContext;
use OpenTelemetry\API\Trace\SpanExporter\Memory;

Memory::reset();
MemoryMetricsExporter::reset();
MemoryLogsExporter::reset();

$tracer = Globals::tracerProvider()->getTracer(
    'limits',
    attributes: ['scope.a' => 'abcdef', 'scope.b' => 2, 'scope.c' => 3],
);
$span = $tracer->spanBuilder('limited')->startSpan();
$span->setAttributes([
    'unicode' => 'åäöxyz',
    'str_arr' => ['abcdef', 'xy', 'third'],
    'numeric' => [1, 2.5, 3],
    'mixed' => ['x', 1],
    'nested' => [[1]],
    'empty' => [],
    'a' => '1',
    'b' => '2',
    'c' => '3',
    'd' => '4',
    'e' => 'dropped-by-count',
    'too-long-key' => 'dropped-by-key',
]);
$span->addEvent('event-1', ['ev.a' => 'abcdef', 'ev.b' => 2, 'ev.c' => 3]);
$span->addEvent('event-2');
$span->addEvent('event-3');
$link = SpanContext::create(
    '2b4ef3412d587ce6e7880fb27a316b8c',
    '7480a670201f6340',
);
$span->addLink($link, ['ln.a' => 'abcdef', 'ln.b' => 2, 'ln.c' => 3]);
$span->addLink($link, ['ln.a' => 'second']);
$span->end();

$exported = Memory::getSpans()[0];
$attributes = $exported['attributes'];
echo 'trace=', count($attributes), "\n";
echo 'unicode=', $attributes['unicode'], "\n";
echo 'strings=', implode(',', $attributes['str_arr']), "\n";
echo 'numeric=', implode(',', $attributes['numeric']), "\n";
echo 'invalid=', isset($attributes['mixed']) ? 'yes' : 'no', ',', isset($attributes['nested']) ? 'yes' : 'no', ',', isset($attributes['too-long-key']) ? 'yes' : 'no', ',', isset($attributes['e']) ? 'yes' : 'no', "\n";
echo 'events=', count($exported['events']), ':', $exported['events'][0]['name'], ':', count($exported['events'][0]['attributes']), ':', $exported['events'][0]['attributes']['ev.a'], "\n";
echo 'links=', count($exported['links']), ':', count($exported['links'][0]['attributes']), ':', $exported['links'][0]['attributes']['ln.a'], "\n";
echo 'scope=', count($exported['instrumentation_scope']['attributes']), ':', $exported['instrumentation_scope']['attributes']['scope.a'], "\n";

$counter = Globals::meterProvider()->getMeter('limits')->createCounter('requests');
$counter->add(1, ['m.a' => 'abcdef', 'm.b' => 2, 'm.c' => 3]);
MemoryMetricsExporter::forceFlush();
$metricAttributes = MemoryMetricsExporter::getMetrics()[0]['data_points'][0]['attributes'];
echo 'metric=', count($metricAttributes), ':', $metricAttributes['m.a'], "\n";

$record = (new LogRecord())
    ->setAttributes(['l.a' => 'abcdef', 'l.b' => [1, 2, 3], 'l.c' => 'dropped'])
    ->setAttribute('l.a', 'uvwxyz');
Globals::loggerProvider()->getLogger('limits')->emit($record);
$logAttributes = MemoryLogsExporter::getLogs()[0]['attributes'];
echo 'log=', count($logAttributes), ':', $logAttributes['l.a'], ':', $logAttributes['l.b'], "\n";
?>
--EXPECT--
trace=8
unicode=åäö
strings=abc,xy
numeric=1,2.5
invalid=no,no,no,no
events=2:event-1:2:abc
links=1:2:abc
scope=2:abcd
metric=3:abcdef
log=2:String(Owned("uvwx")):ListAny([Int(1), Int(2)])
