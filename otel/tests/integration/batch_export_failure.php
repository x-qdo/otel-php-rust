<?php

declare(strict_types=1);

use OpenTelemetry\API\Globals;

$provider = Globals::tracerProvider();
$span = $provider
    ->getTracer('batch-export-failure')
    ->spanBuilder('connection-refused')
    ->startSpan();
$span->end();

$started = hrtime(true);
$provider->forceFlush();
$elapsedMs = (hrtime(true) - $started) / 1_000_000;
$metrics = $provider->getRuntimeMetrics();

$expected = [
    'sampled_ended' => 1,
    'queued' => 1,
    'exported' => 0,
    'dropped_queue_full' => 0,
    'dropped_export_failure' => 1,
    'dropped_shutdown' => 0,
    'export_failures' => 1,
    'queue_depth' => 0,
    'in_flight' => 0,
];
foreach ($expected as $key => $value) {
    if (($metrics[$key] ?? null) !== $value) {
        throw new RuntimeException(sprintf(
            'Expected runtime metric %s=%d, got %s',
            $key,
            $value,
            var_export($metrics[$key] ?? null, true),
        ));
    }
}
if ($elapsedMs > 1_000) {
    throw new RuntimeException(sprintf('Force flush exceeded bound: %.3f ms', $elapsedMs));
}

echo json_encode([
    'force_flush_ms' => $elapsedMs,
    'metrics' => $metrics,
], JSON_THROW_ON_ERROR), "\n";
