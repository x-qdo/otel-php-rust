<?php

declare(strict_types=1);

use OpenTelemetry\API\Globals;

$chunks = max(2, (int) (getenv('OTEL_LEAK_CHUNKS') ?: 10));
$spansPerChunk = max(1, (int) (getenv('OTEL_LEAK_SPANS_PER_CHUNK') ?: 10000));
$operation = getenv('OTEL_LEAK_OPERATION') ?: 'end';
$tracer = Globals::tracerProvider()->getTracer('long-worker-test');

$rssKiB = static function (): int {
    $status = file_get_contents('/proc/self/status') ?: '';
    if (preg_match('/^VmRSS:\s+(\d+)/m', $status, $matches) !== 1) {
        throw new RuntimeException('Unable to read process RSS');
    }

    return (int) $matches[1];
};

$samples = [];
for ($chunk = 0; $chunk < $chunks; ++$chunk) {
    for ($i = 0; $i < $spansPerChunk; ++$i) {
        $builder = $tracer->spanBuilder('worker-operation');
        if ($operation === 'builder') {
            unset($builder);
            continue;
        }
        $span = $builder->startSpan();
        unset($builder);
        if ($operation === 'end') {
            $span->end();
        }
        unset($span);
    }
    gc_collect_cycles();
    $samples[] = $rssKiB();
}

$growthAfterWarmupKiB = $samples[array_key_last($samples)] - $samples[0];
echo json_encode([
    'chunks' => $chunks,
    'spans_per_chunk' => $spansPerChunk,
    'operation' => $operation,
    'rss_kib' => $samples,
    'growth_after_warmup_kib' => $growthAfterWarmupKiB,
], JSON_THROW_ON_ERROR), "\n";

if ($growthAfterWarmupKiB > 8192) {
    fwrite(STDERR, sprintf(
        "RSS grew by %d KiB after the first chunk; limit is 8192 KiB\n",
        $growthAfterWarmupKiB,
    ));
    exit(1);
}
