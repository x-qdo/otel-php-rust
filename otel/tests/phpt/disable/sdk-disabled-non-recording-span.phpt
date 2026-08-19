--TEST--
Disabled SDK yields non-recording spans with invalid contexts and a working scope API
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
OTEL_SDK_DISABLED=true
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\NonRecordingSpan;
use OpenTelemetry\API\Trace\Span;
use OpenTelemetry\API\Trace\SpanExporter\Memory;
use OpenTelemetry\Context\Context;
use OpenTelemetry\Context\ContextInterface;
use OpenTelemetry\Context\ScopeInterface;

$tracer = Globals::tracerProvider()->getTracer('disabled-test', '0.1', 'schema.url', ['a' => 'b']);
$span = $tracer->spanBuilder('root')
    ->setAttribute('a', 1)
    ->setAttributes(['b' => 'c'])
    ->setSpanKind(2)
    ->setStartTimestamp(1)
    ->startSpan();
var_dump($span instanceof NonRecordingSpan);
var_dump($span->isRecording());
var_dump($span->getContext()->isValid());

$scope = $span->activate();
var_dump($scope instanceof ScopeInterface);
var_dump(Span::getCurrent()->getContext()->isValid());
$span->setAttribute('b', 2)->addEvent('e', ['k' => 'v'])->setStatus('Ok')->updateName('renamed')->end();
var_dump($scope->detach());

$context = $span->storeInContext(Context::getCurrent());
var_dump($context instanceof ContextInterface);
var_dump(Span::fromContext($context)->getContext()->isValid());
var_dump(NonRecordingSpan::fromContext($context)->getContext()->isValid());
var_dump(NonRecordingSpan::getCurrent()->getContext()->isValid());
var_dump(Memory::count());
?>
--EXPECT--
bool(true)
bool(false)
bool(false)
bool(true)
bool(false)
int(0)
bool(true)
bool(false)
bool(false)
bool(false)
int(0)
