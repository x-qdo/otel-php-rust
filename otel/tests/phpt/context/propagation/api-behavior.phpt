--TEST--
Context propagation getter, setter, and multi-propagator interoperate
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=none
OTEL_LOGS_EXPORTER=none
OTEL_METRICS_EXPORTER=none
--FILE--
<?php
use OpenTelemetry\Context\{Context, ContextInterface, ContextKeyInterface};
use OpenTelemetry\Context\Propagation\{
    ArrayAccessGetterSetter,
    MultiTextMapPropagator,
    PropagationGetterInterface,
    PropagationSetterInterface,
    TextMapPropagatorInterface
};

final class TestPropagator implements TextMapPropagatorInterface
{
    public function __construct(
        private string $field,
        private ContextKeyInterface $key,
    ) {}

    public function fields(): array { return [$this->field]; }

    public function inject(
        mixed &$carrier,
        ?PropagationSetterInterface $setter = null,
        ?ContextInterface $context = null,
    ): void {
        $setter ??= ArrayAccessGetterSetter::getInstance();
        $context ??= Context::getCurrent();
        $value = $context->get($this->key);
        if (is_string($value)) {
            $setter->set($carrier, $this->field, $value);
        }
    }

    public function extract(
        $carrier,
        ?PropagationGetterInterface $getter = null,
        ?ContextInterface $context = null,
    ): ContextInterface {
        $getter ??= ArrayAccessGetterSetter::getInstance();
        $context ??= Context::getCurrent();
        return $context->with($this->key, $getter->get($carrier, $this->field));
    }
}

$accessor = ArrayAccessGetterSetter::getInstance();
var_dump($accessor === ArrayAccessGetterSetter::getInstance());
$headers = ['TraceParent' => ['first', 'second']];
var_dump($accessor->get($headers, 'traceparent'));
var_dump($accessor->getAll($headers, 'TRACEPARENT'));
$accessor->set($headers, 'traceparent', 'updated');
var_dump($headers);

$object = new ArrayObject(['X-Test' => 'object']);
var_dump($accessor->get($object, 'x-test'));
$accessor->set($object, 'X-New', 'value');
var_dump($object['X-New']);

$firstKey = Context::createKey('first');
$secondKey = Context::createKey('second');
$multi = new MultiTextMapPropagator([
    new TestPropagator('x-first', $firstKey),
    new TestPropagator('x-second', $secondKey),
]);
var_dump($multi->fields());
$context = Context::getRoot()->with($firstKey, 'one')->with($secondKey, 'two');
$carrier = [];
$multi->inject($carrier, null, $context);
var_dump($carrier);
$extracted = $multi->extract($carrier, null, Context::getRoot());
var_dump($extracted->get($firstKey), $extracted->get($secondKey));
?>
--EXPECT--
bool(true)
string(5) "first"
array(2) {
  [0]=>
  string(5) "first"
  [1]=>
  string(6) "second"
}
array(1) {
  ["traceparent"]=>
  string(7) "updated"
}
string(6) "object"
string(5) "value"
array(2) {
  [0]=>
  string(7) "x-first"
  [1]=>
  string(8) "x-second"
}
array(2) {
  ["x-first"]=>
  string(3) "one"
  ["x-second"]=>
  string(3) "two"
}
string(3) "one"
string(3) "two"
