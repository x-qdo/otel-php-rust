--TEST--
Local root span
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=none
--INI--
otel.cli.enabled=1
otel.log.level="error"
otel.log.file="/dev/stdout"
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\LocalRootSpan;
use OpenTelemetry\API\Trace\NonRecordingSpan;

$tracer = Globals::tracerProvider()->getTracer('my_tracer', '0.1', 'schema.url');

//initially, there is no local root span
$invalid = LocalRootSpan::current();
var_dump($invalid instanceof NonRecordingSpan, $invalid->getContext()->isValid());

$root = $tracer->spanBuilder('root')->startSpan();
$scope = $root->activate();
$localRoot = LocalRootSpan::current();
assert(LocalRootSpan::current()->getContext()->getSpanId() === $root->getContext()->getSpanId());
var_dump($localRoot === $root, $localRoot->getContext()->isValid());
$root->end();
$scope->detach();

//there should be no local root span
$invalid = LocalRootSpan::current();
var_dump($invalid instanceof NonRecordingSpan, $invalid->getContext()->isValid());
?>
--EXPECT--
bool(true)
bool(false)
bool(true)
bool(true)
bool(true)
bool(false)
