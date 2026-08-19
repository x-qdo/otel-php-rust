--TEST--
Internal context storage empty after use
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_LOGS_EXPORTER=none
OTEL_METRICS_EXPORTER=none
--INI--
otel.log.level=debug
otel.log.file="/dev/stdout"
otel.cli.enabled=1
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\Span;
use OpenTelemetry\Context\Context;

$tracer = Globals::tracerProvider()->getTracer('my_tracer', '0.1', 'schema.url');

// "pre hook"
var_dump('pre: start span');
$span = $tracer->spanBuilder('root')->startSpan();
var_dump('post: start span');
var_dump('pre: get current context');
$context = Context::getCurrent();
var_dump('post: get current context');
var_dump('pre: storage attach');
Context::storage()->attach($span->storeInContext($context));
var_dump('post: storage attach');
unset($span);

// "post hook"
var_dump('pre: get scope from storage');
$scope = Context::storage()->scope();
var_dump('post: get scope from storage');
var_dump('pre: detach scope');
$scope->detach();
var_dump('post: detach scope');
var_dump('pre: get span from scope context');
$span = Span::fromContext($scope->context());
var_dump('post: get span from scope context');
var_dump('span remains recording after detach', $span->isRecording());
var_dump('pre: unset scope');
unset($scope);
var_dump('post: unset scope');
var_dump('pre: span end');
$span->end();
var_dump('post: span end');

?>
--EXPECTREGEX--
(?s).*string\(15\) "pre: start span".*string\(16\) "post: start span".*string\(19\) "pre: storage attach".*string\(20\) "post: storage attach".*string\(17\) "pre: detach scope".*string\(18\) "post: detach scope".*string\(35\) "span remains recording after detach"\s+bool\(true\).*string\(13\) "pre: span end".*string\(14\) "post: span end".*message=RSHUTDOWN::CONTEXT_STORAGE is empty :\).*
