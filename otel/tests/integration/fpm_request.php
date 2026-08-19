<?php
// One traced FPM request: a root span with a child span, both ended before the
// response. The shell test counts these spans at the collector.
use OpenTelemetry\API\Globals;

$tracer = Globals::tracerProvider()->getTracer('fpm-lifecycle');
$root = $tracer->spanBuilder('fpm-request')->setAttribute('fpm.pid', getmypid())->startSpan();
$scope = $root->activate();
$child = $tracer->spanBuilder('fpm-work')->startSpan();
$child->end();
$scope->detach();
$root->end();
header('Content-Type: text/plain');
echo json_encode(['pid' => getmypid(), 'metrics' => Globals::tracerProvider()->getRuntimeMetrics()]), "\n";
