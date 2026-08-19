--TEST--
Context storage supports scopes, explicit resolution, and custom storage
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=none
OTEL_LOGS_EXPORTER=none
OTEL_METRICS_EXPORTER=none
--FILE--
<?php
use OpenTelemetry\Context\{
    Context,
    ContextInterface,
    ContextStorageInterface,
    ContextStorageScopeInterface,
    ExecutionContextAwareInterface
};

final class TestStorage implements ContextStorageInterface, ExecutionContextAwareInterface
{
    public function __construct(private ContextInterface $current) {}
    public function current(): ContextInterface { return $this->current; }
    public function attach(ContextInterface $context): ContextStorageScopeInterface
    {
        throw new LogicException('not used');
    }
    public function scope(): ?ContextStorageScopeInterface { return null; }
    public function fork(int|string $id): void {}
    public function switch(int|string $id): void {}
    public function destroy(int|string $id): void {}
}

$key = Context::createKey('storage-key');
$native = Context::storage();
var_dump($native instanceof ContextStorageInterface);
var_dump($native instanceof ExecutionContextAwareInterface);

$context = Context::getRoot()->with($key, 'native');
$scope = $native->attach($context);
var_dump($native->scope() instanceof ContextStorageScopeInterface);
var_dump($native->scope()->context()->get($key));
$scope['request'] = 42;
var_dump(isset($scope['request']), $scope['request']);
unset($scope['request']);
var_dump(isset($scope['request']));
var_dump($scope->context()->get($key));
$scope->detach();

$customContext = Context::getRoot()->with($key, 'custom');
$custom = new TestStorage($customContext);
Context::setStorage($custom);
var_dump(Context::storage() === $custom);
var_dump(Context::getCurrent() === $customContext);
var_dump(Context::resolve(null) === $customContext);
var_dump(Context::resolve(null, $custom) === $customContext);
var_dump(Context::resolve(false)->get($key));
var_dump(Context::resolve($context) === $context);
Context::setStorage($native);
?>
--EXPECT--
bool(true)
bool(true)
bool(true)
string(6) "native"
bool(true)
int(42)
bool(false)
string(6) "native"
bool(true)
bool(true)
bool(true)
bool(true)
NULL
bool(true)
