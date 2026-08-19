<?php

declare(strict_types=1);

use OpenTelemetry\API\Globals;

$provider = Globals::tracerProvider();
$span = $provider
    ->getTracer('otlp-auth-header-test')
    ->spanBuilder('authenticated-export')
    ->startSpan();
$span->setAttribute('auth.header.source', getenv('AUTH_HEADER_SOURCE'));
$span->end();
$provider->forceFlush();

$metrics = $provider->getRuntimeMetrics();
foreach ([
    'sampled_ended' => 1,
    'queued' => 1,
    'exported' => 1,
    'dropped_export_failure' => 0,
    'export_failures' => 0,
] as $key => $expected) {
    if (($metrics[$key] ?? null) !== $expected) {
        throw new RuntimeException(sprintf(
            'Expected runtime metric %s=%d, got %s',
            $key,
            $expected,
            var_export($metrics[$key] ?? null, true),
        ));
    }
}

echo json_encode($metrics, JSON_THROW_ON_ERROR), "\n";
