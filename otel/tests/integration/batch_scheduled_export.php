<?php

declare(strict_types=1);

use OpenTelemetry\API\Globals;

$provider = Globals::tracerProvider();
$span = $provider
    ->getTracer('batch-schedule-test')
    ->spanBuilder('partial-batch')
    ->startSpan();
$span->end();

$deadline = hrtime(true) + 1_000_000_000;
do {
    $metrics = $provider->getRuntimeMetrics();
    if ($metrics['exported'] === 1) {
        break;
    }
    usleep(10_000);
} while (hrtime(true) < $deadline);

foreach ([
    'sampled_ended' => 1,
    'queued' => 1,
    'exported' => 1,
    'dropped_queue_full' => 0,
    'dropped_export_failure' => 0,
    'export_failures' => 0,
    'queue_depth' => 0,
    'in_flight' => 0,
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
