<?php

declare(strict_types=1);

use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\SpanContext;
use OpenTelemetry\API\Trace\StatusCode;

$provider = Globals::tracerProvider();
$tracer = $provider->getTracer(
    'otel-rust-conformance',
    '1.2.3',
    'https://example.test/schema',
    ['scope.attribute' => 'preserved'],
);

$root = $tracer
    ->spanBuilder('conformance-root')
    ->setSpanKind(1)
    ->startSpan();
$root->setAttributes([
    'request.id' => 'request-123',
    'request.cached' => false,
]);
$scope = $root->activate();

for ($i = 0; $i < 5; ++$i) {
    $child = $tracer
        ->spanBuilder('conformance-child-' . $i)
        ->setSpanKind(2)
        ->startSpan();
    $child->setAttributes([
        'iteration' => $i,
        'cache.hit' => $i % 2 === 0,
        'duration.factor' => 1.5,
        'labels' => ['alpha', 'beta'],
        'counts' => [1, 2, 3],
    ]);
    $child->addEvent('queue.receive', [
        'attempt' => $i + 1,
        'redelivered' => false,
    ]);

    if ($i === 0) {
        $child->addLink(
            SpanContext::create(
                '2b4ef3412d587ce6e7880fb27a316b8c',
                '7480a670201f6340',
            ),
            ['link.kind' => 'retry', 'link.attempt' => 2],
        );
        try {
            throw new RuntimeException('export failure example');
        } catch (RuntimeException $exception) {
            $child
                ->recordException($exception)
                ->setStatus(StatusCode::STATUS_ERROR, 'child failed');
        }
    }

    $child->end();
}

$scope->detach();
$root->end();
$provider->forceFlush();

$metrics = $provider->getRuntimeMetrics();
$expectedMetrics = [
    'sampled_ended' => 6,
    'queued' => 6,
    'exported' => 6,
    'dropped_queue_full' => 0,
    'dropped_export_failure' => 0,
    'dropped_shutdown' => 0,
    'export_failures' => 0,
    'queue_depth' => 0,
    'in_flight' => 0,
];
foreach ($expectedMetrics as $key => $value) {
    if (($metrics[$key] ?? null) !== $value) {
        throw new RuntimeException(sprintf(
            'Expected runtime metric %s=%d, got %s',
            $key,
            $value,
            var_export($metrics[$key] ?? null, true),
        ));
    }
}
if ($metrics['queue_high_watermark'] < 1 || $metrics['queue_high_watermark'] > 16) {
    throw new RuntimeException('Queue high-water mark is outside the configured bound');
}

echo json_encode($metrics, JSON_THROW_ON_ERROR), "\n";
