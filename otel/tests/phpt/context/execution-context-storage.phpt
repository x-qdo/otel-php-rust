--TEST--
Execution-aware storage isolates and restores forked context stacks
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=none
OTEL_LOGS_EXPORTER=none
OTEL_METRICS_EXPORTER=none
--FILE--
<?php
use OpenTelemetry\Context\{Context, ScopeInterface};

$key = Context::createKey('execution');
$storage = Context::storage();
$main = Context::getRoot()->with($key, 'main');
$mainScope = $storage->attach($main);

$storage->fork(7);
$storage->switch(7);
var_dump($storage->current()->get($key));
$fork = $storage->current()->with($key, 'fork');
$forkScope = $storage->attach($fork);
var_dump($storage->current()->get($key));
var_dump($mainScope->detach() === ScopeInterface::INACTIVE);

$storage->switch('missing');
var_dump($storage->current()->get($key));
var_dump($forkScope->detach() === ScopeInterface::INACTIVE);
var_dump($mainScope->detach());
var_dump($storage->current()->get($key));

$storage->switch(7);
var_dump($storage->current()->get($key));
var_dump($forkScope->detach());
$storage->switch('missing');
$storage->destroy(7);
var_dump($storage->current()->get($key));
?>
--EXPECT--
string(4) "main"
string(4) "fork"
bool(true)
string(4) "main"
bool(true)
int(0)
NULL
string(4) "fork"
int(0)
NULL
