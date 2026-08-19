<?php

declare(strict_types=1);

$engine = getenv('OTEL_BENCH_ENGINE') ?: (extension_loaded('otel') ? 'rust-fork' : 'unknown');
$protocol = getenv('OTEL_EXPORTER_OTLP_PROTOCOL') ?: 'unknown';
$bootstrap = getenv('OTEL_BENCH_PROVIDER_BOOTSTRAP');
$provider = $bootstrap !== false && $bootstrap !== ''
    ? require $bootstrap
    : OpenTelemetry\API\Globals::tracerProvider();
$tracer = $provider->getTracer('scheduled-blackhole-benchmark');

$warmupSpan = $tracer->spanBuilder('warmup-span')->startSpan();
$warmupSpanRecording = $warmupSpan->isRecording();
$warmupSpan->end();
$scheduleDelayMilliseconds = (int) (getenv('OTEL_BSP_SCHEDULE_DELAY') ?: 1000);
usleep(($scheduleDelayMilliseconds + 250) * 1000);

$triggerSpan = $tracer->spanBuilder('trigger-span')->startSpan();
$triggerSpanRecording = $triggerSpan->isRecording();
$startedAt = hrtime(true);
$triggerSpan->end();
$elapsedNanoseconds = hrtime(true) - $startedAt;

echo json_encode([
    'engine' => $engine,
    'protocol' => $protocol,
    'warmup_span_ended' => true,
    'warmup_span_recording' => $warmupSpanRecording,
    'trigger_span_recording' => $triggerSpanRecording,
    'waited_past_schedule' => true,
    'trigger_span_end_elapsed_ms' => $elapsedNanoseconds / 1_000_000,
    'operation_contract_sha256' => hash_file('sha256', __FILE__),
], JSON_THROW_ON_ERROR), "\n";
