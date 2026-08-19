<?php

declare(strict_types=1);

use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\Span;
use OpenTelemetry\Context\Context;

// A long-running Messenger-style worker: every unit of work extracts the upstream trace
// context from message headers, starts a CONSUMER span as root of the unit, activates it,
// does traced work in a child span, detaches and ends. Across many units the process must
// keep a flat RSS and leave no active context behind: the next unit starts from the root
// context again, exactly like a fresh request.
$chunks = max(2, (int) (getenv('OTEL_UNIT_CHUNKS') ?: 10));
$unitsPerChunk = max(1, (int) (getenv('OTEL_UNITS_PER_CHUNK') ?: 2000));
$provider = Globals::tracerProvider();
$tracer = $provider->getTracer('messenger-worker');
$propagator = Globals::propagator();

$rssKiB = static function (): int {
    $status = file_get_contents('/proc/self/status') ?: '';
    if (preg_match('/^VmRSS:\s+(\d+)/m', $status, $matches) !== 1) {
        throw new RuntimeException('Unable to read process RSS');
    }

    return (int) $matches[1];
};

$fail = static function (string $message): never {
    fwrite(STDERR, $message . "\n");
    exit(1);
};

$samples = [];
$unit = 0;
for ($chunk = 0; $chunk < $chunks; ++$chunk) {
    for ($i = 0; $i < $unitsPerChunk; ++$i, ++$unit) {
        $headers = [
            'traceparent' => sprintf('00-%032x-%016x-01', $unit + 1, $unit + 1),
            'tracestate' => 'vendor=unit' . $unit,
        ];
        $parent = $propagator->extract($headers);
        $consumer = $tracer->spanBuilder('message.consume')
            ->setParent($parent)
            ->setSpanKind(4) // SpanKind::KIND_CONSUMER
            ->startSpan();
        $scope = $consumer->activate();
        try {
            if (Span::getCurrent()->getContext()->getSpanId() !== $consumer->getContext()->getSpanId()) {
                $fail("unit {$unit}: activated span is not current");
            }
            $handler = $tracer->spanBuilder('handler.handle')->startSpan();
            $handler->setAttribute('messenger.unit', $unit);
            if ($unit % 7 === 0) {
                // Handlers throw; the worker records and continues with the next unit.
                try {
                    throw new RuntimeException('handler failure');
                } catch (RuntimeException $e) {
                    $handler->recordException($e);
                }
            }
            $handler->end();
        } finally {
            $scope->detach();
            $consumer->end();
        }
        if (Span::getCurrent()->getContext()->isValid()) {
            $fail("unit {$unit}: a span leaked into the current context after detach");
        }
        if (Context::storage()->scope() !== null) {
            $fail("unit {$unit}: a context scope leaked after detach");
        }
        // Occasional carrier injection, as a dispatching handler would do.
        if ($unit % 5 === 0) {
            $carrier = [];
            $propagator->inject($carrier);
        }
    }
    gc_collect_cycles();
    $samples[] = $rssKiB();
}

$growthKiB = $samples[array_key_last($samples)] - $samples[0];
$metrics = $provider->getRuntimeMetrics();
echo json_encode([
    'chunks' => $chunks,
    'units_per_chunk' => $unitsPerChunk,
    'rss_kib' => $samples,
    'growth_after_warmup_kib' => $growthKiB,
    'metrics' => $metrics,
], JSON_THROW_ON_ERROR), "\n";

if ($growthKiB > 8192) {
    $fail(sprintf('RSS grew by %d KiB after the first chunk; limit is 8192 KiB', $growthKiB));
}
if ($metrics['sampled_started'] !== $chunks * $unitsPerChunk * 2) {
    $fail(sprintf('expected %d sampled spans, got %d', $chunks * $unitsPerChunk * 2, $metrics['sampled_started']));
}
