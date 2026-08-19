--TEST--
Set and retrieve a value from context
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=none
OTEL_LOGS_EXPORTER=none
OTEL_METRICS_EXPORTER=none
--FILE--
<?php
use OpenTelemetry\Context\{Context, ContextKey, ContextKeys};

$some = Context::createKey('some_key');
$another = new ContextKey('another_key');
$objectKey = new ContextKey();
$root = Context::getRoot();
$context = $root->with($some, 'A');
$context2 = $context->with($another, ['B']);
$value = new stdClass();
$context3 = $context2->with($objectKey, $value);

var_dump($root->get($some));
var_dump($context3->get($some));
var_dump($context3->get($another));
var_dump($context3->get($objectKey) === $value);
var_dump($context3->with($some, null)->get($some));
var_dump($context3->get($some));
var_dump(ContextKeys::span() === ContextKeys::span());
var_dump(ContextKeys::baggage() === ContextKeys::baggage());
?>
--EXPECT--
NULL
string(1) "A"
array(1) {
  [0]=>
  string(1) "B"
}
bool(true)
NULL
string(1) "A"
bool(true)
bool(true)
