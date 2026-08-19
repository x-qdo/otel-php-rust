--TEST--
A SpanContext created without its native state behaves as the invalid span context
--DESCRIPTION--
The class is final, so Reflection refuses newInstanceWithoutConstructor(); unserialize()
still creates the object through create_object without running create(), leaving the
native state empty.
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
--FILE--
<?php
use OpenTelemetry\API\Trace\SpanContext;

$context = unserialize('O:' . strlen(SpanContext::class) . ':"' . SpanContext::class . '":0:{}');
var_dump($context->isValid());
var_dump($context->getTraceId());
var_dump($context->getSpanId());
var_dump($context->isRemote());
var_dump($context->isSampled());
echo "done\n";
?>
--EXPECT--
bool(false)
string(32) "00000000000000000000000000000000"
string(16) "0000000000000000"
bool(false)
bool(false)
done
