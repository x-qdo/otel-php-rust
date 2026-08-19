<?php

declare(strict_types=1);

$iterations = max(1, (int) (getenv('OTEL_BENCH_ITERATIONS') ?: 100000));
$warmupIterations = max(100, intdiv($iterations, 10));
$mode = getenv('OTEL_BENCH_MODE') ?: 'unknown';
$engine = getenv('OTEL_BENCH_ENGINE') ?: (extension_loaded('otel') ? 'rust-fork' : 'baseline');
$providerSetupStartedAt = hrtime(true);
$bootstrap = getenv('OTEL_BENCH_PROVIDER_BOOTSTRAP');
$provider = $bootstrap !== false && $bootstrap !== ''
    ? require $bootstrap
    : (extension_loaded('otel') ? OpenTelemetry\API\Globals::tracerProvider() : null);
$tracer = $provider?->getTracer('manual-benchmark');
$providerSetupElapsedNanoseconds = hrtime(true) - $providerSetupStartedAt;
$recordingSpans = 0;

$run = static function (int $count) use ($tracer, &$recordingSpans): void {
    for ($i = 0; $i < $count; ++$i) {
        if ($tracer === null) {
            continue;
        }

        $span = $tracer
            ->spanBuilder('manual-operation')
            ->setAttribute('operation.index', $i)
            ->startSpan();
        if ($span->isRecording()) {
            ++$recordingSpans;
            $span->setAttributes([
                'operation.kind' => 'benchmark',
                'operation.cached' => false,
                'operation.factor' => 1.5,
            ]);
        }
        $span->end();
    }
};

$run($warmupIterations);
$recordingSpans = 0;
$startedAt = hrtime(true);
$run($iterations);
$elapsedNanoseconds = hrtime(true) - $startedAt;
$forceFlushStartedAt = hrtime(true);
$forceFlushResult = $provider !== null && method_exists($provider, 'forceFlush')
    ? $provider->forceFlush()
    : true;
$forceFlushElapsedNanoseconds = hrtime(true) - $forceFlushStartedAt;
$runtimeMetrics = $provider !== null && method_exists($provider, 'getRuntimeMetrics')
    ? $provider->getRuntimeMetrics()
    : null;
if (
    $runtimeMetrics !== null
    && $runtimeMetrics['queue_high_watermark'] > (int) (getenv('OTEL_BSP_MAX_QUEUE_SIZE') ?: 2048)
) {
    throw new RuntimeException('Observed queue depth exceeded its configured bound');
}

$status = file_get_contents('/proc/self/status') ?: '';
$readStatus = static function (string $name) use ($status): ?int {
    if (preg_match('/^' . preg_quote($name, '/') . ':\s+(\d+)/m', $status, $matches) !== 1) {
        return null;
    }

    return (int) $matches[1];
};

echo json_encode([
    'engine' => $engine,
    'mode' => $mode,
    'iterations' => $iterations,
    'warmup_iterations' => $warmupIterations,
    'recording_spans' => $recordingSpans,
    'operation_contract_sha256' => hash_file('sha256', __FILE__),
    'provider_setup_elapsed_ms' => $providerSetupElapsedNanoseconds / 1_000_000,
    'loop_elapsed_ms' => $elapsedNanoseconds / 1_000_000,
    'elapsed_ms' => $elapsedNanoseconds / 1_000_000,
    'ns_per_operation' => $elapsedNanoseconds / $iterations,
    'force_flush_elapsed_ms' => $forceFlushElapsedNanoseconds / 1_000_000,
    'force_flush_result' => $forceFlushResult,
    'rss_kib' => $readStatus('VmRSS'),
    'peak_rss_kib' => $readStatus('VmHWM'),
    'threads' => $readStatus('Threads'),
    'runtime_metrics' => $runtimeMetrics,
], JSON_THROW_ON_ERROR), "\n";
