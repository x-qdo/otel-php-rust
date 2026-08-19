--TEST--
Native metrics export counters, up/down counters, gauges, and histograms
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=none
OTEL_LOGS_EXPORTER=none
OTEL_METRICS_EXPORTER=memory
--INI--
otel.cli.enabled=1
otel.auto.enabled=0
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Metrics\CounterInterface;
use OpenTelemetry\API\Metrics\MemoryMetricsExporter;

$meter = Globals::meterProvider()->getMeter('test.metrics', '1.0', null, ['scope' => 'test']);

$counter = $meter->createCounter('requests', '1', 'request count');
$counter->add(2, ['route' => '/checkout']);

$upDown = $meter->createUpDownCounter('active');
$upDown->add(-1);

$gauge = $meter->createGauge('temperature');
$gauge->record(20.5);

$histogram = $meter->createHistogram(
    'latency',
    'ms',
    advisory: ['ExplicitBucketBoundaries' => [1, 10, 100]],
);
$attributes = (static function (): iterable {
    yield 'status' => 'ok';
})();
$histogram->record(12.5, $attributes);

MemoryMetricsExporter::forceFlush();
$metrics = [];
foreach (MemoryMetricsExporter::getMetrics() as $metric) {
    $metrics[$metric['name']] = $metric;
}

var_dump($counter instanceof CounterInterface, $counter->isEnabled());
echo $metrics['requests']['kind'], ':', $metrics['requests']['data_points'][0]['value'], ':', $metrics['requests']['data_points'][0]['attributes']['route'], "\n";
echo $metrics['active']['kind'], ':', $metrics['active']['data_points'][0]['value'], "\n";
echo $metrics['temperature']['kind'], ':', $metrics['temperature']['data_points'][0]['value'], "\n";
echo $metrics['latency']['kind'], ':', $metrics['latency']['data_points'][0]['count'], ':', $metrics['latency']['data_points'][0]['sum'], ':', implode(',', $metrics['latency']['data_points'][0]['bounds']), ':', $metrics['latency']['data_points'][0]['attributes']['status'], "\n";
?>
--EXPECT--
bool(true)
bool(true)
counter:2:/checkout
up_down_counter:-1
gauge:20.5
histogram:1:12.5:1,10,100:ok
