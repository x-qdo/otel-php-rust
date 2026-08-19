--TEST--
Activate context
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=none
OTEL_LOGS_EXPORTER=none
OTEL_METRICS_EXPORTER=none
--INI--
otel.log.level=debug
otel.log.file="/dev/stdout"
otel.cli.enabled=1
--FILE--
<?php
use OpenTelemetry\Context\Context;

$context = Context::getCurrent();
var_dump("activate context");
$scope = $context->activate();
//context is now stored
var_dump("unsetting context");
unset($context);
//context should not have been removed from storage
var_dump("detaching scope");
$scope->detach();
var_dump("scope detached");
?>
--EXPECTREGEX--
(?s).*string\(16\) "activate context".*string\(17\) "unsetting context".*string\(15\) "detaching scope".*string\(14\) "scope detached".*message=RSHUTDOWN::CONTEXT_STORAGE is empty :\).*
