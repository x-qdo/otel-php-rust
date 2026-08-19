--TEST--
Metrics instruments are disabled when the metrics exporter is none
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=none
OTEL_LOGS_EXPORTER=none
OTEL_METRICS_EXPORTER=none
--INI--
otel.cli.enabled=1
otel.auto.enabled=0
--FILE--
<?php
use OpenTelemetry\API\Globals;

$meter = Globals::meterProvider()->getMeter('test.disabled');
$counter = $meter->createCounter('requests');
$observable = $meter->createObservableGauge(
    'load',
    advisory: static fn () => throw new RuntimeException('must not run'),
);
$token = $observable->observe(static fn () => throw new RuntimeException('must not run'));

var_dump($counter->isEnabled(), $observable->isEnabled());
$counter->add(1);
$token->detach();
?>
--EXPECT--
bool(false)
bool(false)
