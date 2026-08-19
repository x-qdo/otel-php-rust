--TEST--
Scope detach reports mismatch and detachment without corrupting the active stack
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
--ENV--
OTEL_TRACES_EXPORTER=none
OTEL_LOGS_EXPORTER=none
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\Span;
use OpenTelemetry\Context\ScopeInterface;

$tracer = Globals::tracerProvider()->getTracer('scope-contract-test');
$root = $tracer->spanBuilder('root')->startSpan();
$rootScope = $root->activate();
$child = $tracer->spanBuilder('child')->startSpan();
$childScope = $child->activate();
$childId = $child->getContext()->getSpanId();

var_dump($rootScope->detach() === ScopeInterface::MISMATCH);
var_dump(Span::getCurrent()->getContext()->getSpanId() === $childId);
var_dump($childScope->detach());
var_dump($rootScope->detach());
var_dump($rootScope->detach() === ScopeInterface::DETACHED);

$child->end();
$root->end();
?>
--EXPECT--
bool(true)
bool(true)
int(0)
int(0)
bool(true)
