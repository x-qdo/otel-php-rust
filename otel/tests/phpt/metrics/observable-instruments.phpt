--TEST--
Native observable metrics run PHP callbacks safely and support batch/detach
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
use OpenTelemetry\API\Metrics\MemoryMetricsExporter;
use OpenTelemetry\API\Metrics\ObservableCounterInterface;

$meter = Globals::meterProvider()->getMeter('test.async');
$counter = $meter->createObservableCounter(
    'queue',
    null,
    null,
    [],
    static fn ($observer) => $observer->observe(7, ['worker' => 'a']),
);
$gauge = $meter->createObservableGauge('load');
$upDown = $meter->createObservableUpDownCounter('connections');

$batch = $meter->batchObserve(
    static function ($gaugeObserver, $upDownObserver): void {
        $gaugeObserver->observe(0.5);
        $upDownObserver->observe(-2);
    },
    $gauge,
    $upDown,
);
$detached = $gauge->observe(static fn ($observer) => $observer->observe(99, ['detached' => true]));
$detached->detach();

MemoryMetricsExporter::forceFlush();
var_dump($counter instanceof ObservableCounterInterface);
foreach (MemoryMetricsExporter::getMetrics() as $metric) {
    echo $metric['name'], ':', $metric['kind'], ':', $metric['data_points'][0]['value'], "\n";
}
$batch->detach();
?>
--EXPECT--
bool(true)
queue:counter:7
load:gauge:0.5
connections:up_down_counter:-2
