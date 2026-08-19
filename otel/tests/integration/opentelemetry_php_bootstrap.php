<?php

declare(strict_types=1);

use OpenTelemetry\API\Common\Time\SystemClock;
use OpenTelemetry\API\Signals;
use OpenTelemetry\API\Trace\NoopTracerProvider;
use OpenTelemetry\Contrib\Grpc\GrpcTransportFactory;
use OpenTelemetry\Contrib\Otlp\ContentTypes;
use OpenTelemetry\Contrib\Otlp\OtlpHttpTransportFactory;
use OpenTelemetry\Contrib\Otlp\OtlpUtil;
use OpenTelemetry\Contrib\Otlp\SpanExporter;
use OpenTelemetry\SDK\Trace\Sampler\AlwaysOnSampler;
use OpenTelemetry\SDK\Trace\Sampler\ParentBased;
use OpenTelemetry\SDK\Trace\Sampler\TraceIdRatioBasedSampler;
use OpenTelemetry\SDK\Trace\SpanProcessor\BatchSpanProcessor;
use OpenTelemetry\SDK\Trace\TracerProvider;
use OpenTelemetry\SDK\Trace\TracerProviderInterface;

require '/opt/opentelemetry-php/vendor/autoload.php';

$mode = getenv('OTEL_BENCH_MODE') ?: 'disabled';
if ($mode === 'disabled') {
    return new NoopTracerProvider();
}

$protocol = getenv('OTEL_EXPORTER_OTLP_PROTOCOL') ?: 'http/protobuf';
$endpoint = getenv('OTEL_EXPORTER_OTLP_ENDPOINT') ?: match ($protocol) {
    'grpc' => 'http://collector-benchmark:4317',
    default => 'http://collector-benchmark:4318',
};
$timeoutSeconds = max(0.001, (float) (getenv('OTEL_BENCH_EXPORT_TIMEOUT_SECONDS') ?: 3.0));

$transport = match ($protocol) {
    'grpc' => (new GrpcTransportFactory())->create(
        $endpoint . OtlpUtil::method(Signals::TRACE),
        timeout: $timeoutSeconds,
        maxRetries: 0,
    ),
    'http/protobuf' => (new OtlpHttpTransportFactory())->create(
        rtrim($endpoint, '/') . '/v1/traces',
        ContentTypes::PROTOBUF,
        timeout: $timeoutSeconds,
        maxRetries: 0,
    ),
    default => throw new InvalidArgumentException(sprintf('Unsupported benchmark protocol: %s', $protocol)),
};

$processor = new BatchSpanProcessor(
    new SpanExporter($transport),
    SystemClock::create(),
    maxQueueSize: (int) (getenv('OTEL_BSP_MAX_QUEUE_SIZE') ?: 2048),
    scheduledDelayMillis: (int) (getenv('OTEL_BSP_SCHEDULE_DELAY') ?: 1000),
    exportTimeoutMillis: (int) (getenv('OTEL_BSP_EXPORT_TIMEOUT') ?: 3000),
    maxExportBatchSize: (int) (getenv('OTEL_BSP_MAX_EXPORT_BATCH_SIZE') ?: 512),
    autoFlush: true,
);

$sampler = match (getenv('OTEL_TRACES_SAMPLER') ?: 'parentbased_traceidratio') {
    'always_on' => new AlwaysOnSampler(),
    'parentbased_traceidratio' => new ParentBased(
        new TraceIdRatioBasedSampler((float) (getenv('OTEL_TRACES_SAMPLER_ARG') ?: 0.01)),
    ),
    default => throw new InvalidArgumentException('Unsupported benchmark sampler'),
};

/** @var TracerProviderInterface */
return new TracerProvider($processor, $sampler);
