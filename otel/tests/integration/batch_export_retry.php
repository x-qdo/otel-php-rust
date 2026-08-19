<?php

declare(strict_types=1);

use OpenTelemetry\API\Globals;

$provider = Globals::tracerProvider();
$tracer = $provider->getTracer('batch-export-retry');
$maxAttempts = (int) (getenv('OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS') ?: 3);
$batchSize = (int) (getenv('OTEL_BSP_MAX_EXPORT_BATCH_SIZE') ?: 4);
$spanCount = $batchSize * 2;

// The first full batch triggers the worker, whose attempts against the refused
// endpoint fail and back off; the second batch is ended while that retry round
// is in progress, so its Span::end() timings are taken under retry load.
$endTimesMs = [];
for ($i = 0; $i < $spanCount; ++$i) {
    $span = $tracer->spanBuilder('connection-refused-' . $i)->startSpan();
    $started = hrtime(true);
    $span->end();
    $endTimesMs[] = (hrtime(true) - $started) / 1_000_000;
}

// Let the first round run its full backoff schedule (50 ms, 100 ms, +/-20 %)
// before the flush; a pending flush cuts backoffs short by design.
usleep(500_000);
$flushStarted = hrtime(true);
$provider->forceFlush();
$flushMs = (hrtime(true) - $flushStarted) / 1_000_000;
$metrics = $provider->getRuntimeMetrics();

$fail = static function (string $message): never {
    throw new RuntimeException($message);
};

foreach (['sampled_ended' => $spanCount, 'queued' => $spanCount, 'exported' => 0,
    'dropped_queue_full' => 0, 'dropped_shutdown' => 0, 'dropped_export_failure' => $spanCount,
    'export_retry_recovered' => 0, 'queue_depth' => 0, 'in_flight' => 0] as $key => $value) {
    if (($metrics[$key] ?? null) !== $value) {
        $fail(sprintf('Expected runtime metric %s=%d, got %s', $key, $value, var_export($metrics[$key] ?? null, true)));
    }
}
$batches = $metrics['export_failures'];
if ($batches < 2) {
    $fail(sprintf('Expected at least two failed batches, got %d', $batches));
}
$expectedRetries = $batches * ($maxAttempts - 1);
if ($metrics['export_retries'] !== $expectedRetries) {
    $fail(sprintf('Expected export_retries=%d (%d batches x %d retries), got %d', $expectedRetries, $batches, $maxAttempts - 1, $metrics['export_retries']));
}
$invariant = $metrics['exported'] + $metrics['dropped_queue_full'] + $metrics['dropped_export_failure'] + $metrics['dropped_shutdown'];
if ($metrics['sampled_ended'] !== $invariant) {
    $fail(sprintf('Drain invariant violated: sampled_ended=%d, accounted=%d', $metrics['sampled_ended'], $invariant));
}

sort($endTimesMs);
$medianEndMs = $endTimesMs[intdiv(count($endTimesMs), 2)];
$maxEndMs = $endTimesMs[count($endTimesMs) - 1];
if ($medianEndMs >= 1.0) {
    $fail(sprintf('Span::end() median %.3f ms is not sub-millisecond', $medianEndMs));
}
if ($maxEndMs >= 50.0) {
    $fail(sprintf('Span::end() max %.3f ms indicates a wait on the exporter', $maxEndMs));
}

echo json_encode([
    'protocol' => getenv('OTEL_EXPORTER_OTLP_PROTOCOL'),
    'force_flush_ms' => round($flushMs, 3),
    'span_end_median_ms' => round($medianEndMs, 4),
    'span_end_max_ms' => round($maxEndMs, 4),
    'metrics' => $metrics,
], JSON_THROW_ON_ERROR), "\n";
