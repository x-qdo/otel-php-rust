--TEST--
Test internal events logged
--EXTENSIONS--
otel
--INI--
otel.log.level="trace"
otel.log.file="/dev/stdout"
otel.cli.enabled=1
--ENV--
OTEL_TRACES_EXPORTER=console
--FILE--
<?php
use OpenTelemetry\API\Globals;

Globals::tracerProvider()
    ->getTracer('my_tracer', '0.1', 'schema.url')
    ->spanBuilder('root')
    ->startSpan()
    ->end();
Globals::tracerProvider()->forceFlush();
?>
--EXPECTF--
%A
[%s] [DEBUG] [%s] [ThreadId(%d)] %s message=OpenTelemetry::RINIT
%A
[%s] [DEBUG] [%s] [ThreadId(%d)] otel::trace::batch_processor: event src/trace/batch_processor.rs:%d message=BoundedBatchProcessor.ThreadStarted schedule_delay_ms=1000 max_export_batch_size=512 max_queue_size=2048%A
[%s] [DEBUG] [%s] [ThreadId(%d)] otel::trace::batch_processor: event src/trace/batch_processor.rs:%d message=BoundedBatchProcessor.ExportingDueToForceFlush
Spans
Resource
%A
Span #0
%A
[%s] [DEBUG] [pid=%d] [ThreadId(%d)] otel::trace::tracer_provider: event src/trace/tracer_provider.rs:%d message=OpenTelemetry tracer provider flush success
%A
