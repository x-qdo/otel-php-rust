--TEST--
Native classes declared final by the official API reject userland subclasses
--EXTENSIONS--
otel
--FILE--
<?php
var_dump((new ReflectionClass(OpenTelemetry\API\Globals::class))->isFinal());
var_dump((new ReflectionClass(OpenTelemetry\API\Trace\SpanContext::class))->isFinal());
var_dump((new ReflectionClass(OpenTelemetry\API\Trace\NonRecordingSpan::class))->isFinal());

eval('class MyGlobals extends OpenTelemetry\API\Globals {}');
?>
--EXPECTF--
bool(true)
bool(true)
bool(true)

Fatal error: Class MyGlobals cannot extend final class OpenTelemetry\API\Globals in %s : eval()'d code on line %d
