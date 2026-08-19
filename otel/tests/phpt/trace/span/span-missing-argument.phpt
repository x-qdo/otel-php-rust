--TEST--
Calling a span method without its required argument throws ArgumentCountError instead of aborting
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\SpanExporter\Memory;
use OpenTelemetry\API\Trace\SpanContext;

$span = Globals::tracerProvider()->getTracer('args-test')->spanBuilder('root')->startSpan();
$calls = [
    fn() => $span->setAttribute(),
    fn() => $span->setAttribute('only-key'),
    fn() => $span->setAttributes(),
    fn() => $span->updateName(),
    fn() => $span->addEvent(),
    fn() => $span->addLink(),
    fn() => $span->recordException(),
    fn() => $span->storeInContext(),
    fn() => SpanContext::create(),
    fn() => SpanContext::create('0af7651916cd43dd8448eb211c80319c'),
    fn() => Globals::tracerProvider()->getTracer(),
];
foreach ($calls as $call) {
    try {
        $call();
        echo "no error\n";
    } catch (\ArgumentCountError $e) {
        echo get_class($e), ": ", $e->getMessage(), "\n";
    }
}
$span->setAttribute('k', 'v');
$span->end();
var_dump(Memory::count());
echo "done\n";
?>
--EXPECT--
ArgumentCountError: OpenTelemetry\API\Trace\Span::setAttribute(): expects at least 1 parameter(s), 0 given
ArgumentCountError: OpenTelemetry\API\Trace\Span::setAttribute(): expects at least 2 parameter(s), 1 given
ArgumentCountError: OpenTelemetry\API\Trace\Span::setAttributes(): expects at least 1 parameter(s), 0 given
ArgumentCountError: OpenTelemetry\API\Trace\Span::updateName(): expects at least 1 parameter(s), 0 given
ArgumentCountError: OpenTelemetry\API\Trace\Span::addEvent(): expects at least 1 parameter(s), 0 given
ArgumentCountError: OpenTelemetry\API\Trace\Span::addLink(): expects at least 1 parameter(s), 0 given
ArgumentCountError: OpenTelemetry\API\Trace\Span::recordException(): expects at least 1 parameter(s), 0 given
ArgumentCountError: OpenTelemetry\API\Trace\Span::storeInContext(): expects at least 1 parameter(s), 0 given
ArgumentCountError: OpenTelemetry\API\Trace\SpanContext::create(): expects at least 1 parameter(s), 0 given
ArgumentCountError: OpenTelemetry\API\Trace\SpanContext::create(): expects at least 2 parameter(s), 1 given
ArgumentCountError: OpenTelemetry\API\Trace\TracerProvider::getTracer(): expects at least 1 parameter(s), 0 given
int(1)
done
