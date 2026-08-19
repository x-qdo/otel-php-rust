<?php

declare(strict_types=1);

use OpenTelemetry\API\Globals;

// Generic transport probe: PROBE_PLAN lists export rounds as "<spans>x<attribute bytes>"
// separated by commas. Every round ends spans, force-flushes and snapshots the runtime
// metrics so the shell test can assert wire behaviour against the collector/fixture logs.
$plan = getenv('PROBE_PLAN') ?: '1x0';
$case = getenv('PROBE_CASE') ?: 'default';

$provider = Globals::tracerProvider();
$tracer = $provider->getTracer('otlp-transport-probe');
$rounds = [];

foreach (explode(',', $plan) as $index => $step) {
    [$count, $bytes] = array_map('intval', explode('x', $step));
    $payload = $bytes > 0 ? str_repeat('a', $bytes) : null;
    $maxSpanEndMs = 0.0;
    for ($n = 0; $n < $count; $n++) {
        $span = $tracer
            ->spanBuilder(sprintf('probe-%s-round-%d-span-%d', $case, $index, $n))
            ->startSpan();
        $span->setAttribute('probe.case', $case);
        $span->setAttribute('probe.round', $index);
        if ($payload !== null) {
            $span->setAttribute('probe.payload', $payload);
        }
        $started = hrtime(true);
        $span->end();
        $maxSpanEndMs = max($maxSpanEndMs, (hrtime(true) - $started) / 1_000_000);
    }
    $started = hrtime(true);
    $provider->forceFlush();
    $rounds[] = [
        'spans' => $count,
        'attribute_bytes' => $bytes,
        'max_span_end_ms' => $maxSpanEndMs,
        'force_flush_ms' => (hrtime(true) - $started) / 1_000_000,
        'metrics' => $provider->getRuntimeMetrics(),
    ];
}

echo json_encode(['case' => $case, 'rounds' => $rounds], JSON_THROW_ON_ERROR), "\n";
