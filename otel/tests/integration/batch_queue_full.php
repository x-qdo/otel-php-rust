<?php

declare(strict_types=1);

use OpenTelemetry\API\Globals;

$provider = Globals::tracerProvider();
$tracer = $provider->getTracer('batch-queue-full-test');

// The first span occupies the exporter worker. With a one-span queue, exactly
// one of the following 101 spans can then be queued and the other 100 must be
// dropped without blocking this PHP thread.
$tracer->spanBuilder('in-flight')->startSpan()->end();
usleep(250_000);

$startedAt = hrtime(true);
for ($i = 0; $i < 101; ++$i) {
    $tracer->spanBuilder('queued-' . $i)->startSpan()->end();
}
$elapsedMilliseconds = (hrtime(true) - $startedAt) / 1_000_000;

$metrics = $provider->getRuntimeMetrics();
echo json_encode([
    'elapsed_ms' => $elapsedMilliseconds,
    'metrics' => $metrics,
], JSON_THROW_ON_ERROR), "\n";

if ($elapsedMilliseconds >= 200) {
    fwrite(STDERR, sprintf('Queue-full handoff blocked PHP for %.3f ms', $elapsedMilliseconds));
    exit(1);
}

$expected = [
    'sampled_ended' => 102,
    'queued' => 2,
    'exported' => 0,
    'dropped_queue_full' => 100,
    'export_failures' => 0,
    'queue_depth' => 1,
    'queue_high_watermark' => 1,
];

foreach ($expected as $key => $value) {
    if (($metrics[$key] ?? null) !== $value) {
        fwrite(STDERR, sprintf(
            'Expected %s=%d, got %s',
            $key,
            $value,
            var_export($metrics[$key] ?? null, true),
        ));
        exit(1);
    }
}
