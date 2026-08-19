--TEST--
Get context from scope
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=none
OTEL_LOGS_EXPORTER=none
OTEL_METRICS_EXPORTER=none
--FILE--
<?php
use OpenTelemetry\Context\{Context, ContextStorageScopeInterface};

$key = Context::createKey('some_key');
$context = Context::getCurrent();
$context = $context->with($key, 'A');
$scope = $context->activate();
assert($scope instanceof ContextStorageScopeInterface);

$ctx = $scope->context();
assert($ctx->get($key) === 'A');
assert(Context::getCurrent()->get($key) === 'A');
$scope->detach();
?>
--EXPECT--
