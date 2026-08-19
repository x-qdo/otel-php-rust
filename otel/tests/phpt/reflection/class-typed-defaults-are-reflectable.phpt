--TEST--
Optional class-typed parameters expose their declared default value through Reflection
--DESCRIPTION--
Without the default snippet on the arg_info, Reflection reports the parameter as
optional but without a default, and named-argument calls cannot skip it.
--EXTENSIONS--
otel
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Propagation\TraceContextPropagator;

$context = (new ReflectionMethod(TraceContextPropagator::class, 'inject'))->getParameters()[2];
var_dump($context->getName(), (string) $context->getType(), $context->isOptional(), $context->isDefaultValueAvailable());
var_dump($context->getDefaultValue());

$carrier = [];
Globals::propagator()->inject(carrier: $carrier);
var_dump($carrier);
?>
--EXPECT--
string(7) "context"
string(39) "?OpenTelemetry\Context\ContextInterface"
bool(true)
bool(true)
NULL
array(0) {
}
