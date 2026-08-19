<?php

declare(strict_types=1);

use OpenTelemetry\API\Globals;

// Load generator for the request-thread syscall audit: ends OTEL_AUDIT_SPANS sampled spans as
// fast as possible, records per-span Span::end() latency, flushes, and reports the runtime
// metrics together with the main thread id so the shell test can pick its strace file.
$count = max(100, (int) (getenv('OTEL_AUDIT_SPANS') ?: 20000));
$provider = Globals::tracerProvider();
$tracer = $provider->getTracer('request-thread-syscall-audit');
$latenciesNs = [];

$startedAt = hrtime(true);
for ($i = 0; $i < $count; ++$i) {
    $span = $tracer->spanBuilder('audit-operation')
        ->setAttribute('operation.index', $i)
        ->startSpan();
    $span->setAttributes(['operation.kind' => 'audit', 'operation.factor' => 1.5]);
    $endStartedAt = hrtime(true);
    $span->end();
    $latenciesNs[] = hrtime(true) - $endStartedAt;
}
$loopNs = hrtime(true) - $startedAt;

$flushStartedAt = hrtime(true);
$provider->forceFlush();
$flushNs = hrtime(true) - $flushStartedAt;
// forceFlush() is bounded and returns while a slow or rejecting collector still holds
// batches in flight (timeouts, retries); the drain invariant is defined after the drain, so
// wait (bounded) until the worker owns nothing any more.
$drainStartedAt = hrtime(true);
do {
    $metrics = $provider->getRuntimeMetrics();
    $drained = $metrics['queue_depth'] === 0 && $metrics['in_flight'] === 0;
    if (!$drained) {
        usleep(50_000);
    }
} while (!$drained && (hrtime(true) - $drainStartedAt) < 60_000_000_000);
$drainNs = hrtime(true) - $drainStartedAt;
if (!$drained) {
    fwrite(STDERR, "worker did not drain within 60 s\n");
    exit(1);
}

sort($latenciesNs);
$percentile = static fn (float $p): float => $latenciesNs[(int) min(count($latenciesNs) - 1, floor($p * count($latenciesNs)))] / 1_000_000;
$invariant = $metrics['exported'] + $metrics['dropped_queue_full'] + $metrics['dropped_export_failure'] + $metrics['dropped_shutdown'];
if ($metrics['sampled_ended'] !== $invariant) {
    fwrite(STDERR, sprintf("drain invariant violated: sampled_ended=%d accounted=%d\n", $metrics['sampled_ended'], $invariant));
    exit(1);
}

$status = file_get_contents('/proc/self/status') ?: '';
preg_match('/^Threads:\s+(\d+)/m', $status, $threads);

echo json_encode([
    'case' => getenv('AUDIT_CASE') ?: 'default',
    'pid' => getmypid(),
    'threads' => (int) ($threads[1] ?? 0),
    'spans' => $count,
    'loop_ms' => round($loopNs / 1_000_000, 3),
    'span_end_p50_ms' => round($percentile(0.50), 4),
    'span_end_p99_ms' => round($percentile(0.99), 4),
    'span_end_max_ms' => round(end($latenciesNs) / 1_000_000, 4),
    'force_flush_ms' => round($flushNs / 1_000_000, 3),
    'drain_wait_ms' => round($drainNs / 1_000_000, 3),
    'metrics' => $metrics,
], JSON_THROW_ON_ERROR), "\n";
