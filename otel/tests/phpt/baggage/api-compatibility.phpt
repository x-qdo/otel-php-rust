--TEST--
Native Baggage API supports immutable values, userland contexts, parsing, and W3C propagation
--EXTENSIONS--
otel
--INI--
otel.cli.enabled=1
otel.log.level=error
--FILE--
<?php
use OpenTelemetry\API\Baggage\Baggage;
use OpenTelemetry\API\Baggage\BaggageBuilderInterface;
use OpenTelemetry\API\Baggage\BaggageInterface;
use OpenTelemetry\API\Baggage\Entry;
use OpenTelemetry\API\Baggage\Metadata;
use OpenTelemetry\API\Baggage\MetadataInterface;
use OpenTelemetry\API\Baggage\Propagation\BaggagePropagator;
use OpenTelemetry\API\Baggage\Propagation\Parser;
use OpenTelemetry\Context\Context;
use OpenTelemetry\Context\ContextInterface;
use OpenTelemetry\Context\ContextKeyInterface;
use OpenTelemetry\Context\ImplicitContextKeyedInterface;
use OpenTelemetry\Context\Propagation\PropagationGetterInterface;
use OpenTelemetry\Context\Propagation\PropagationSetterInterface;
use OpenTelemetry\Context\ScopeInterface;

function check(string $name, bool $condition): void
{
    echo $name, ':', $condition ? 'ok' : 'fail', "\n";
}

final class UserContext implements ContextInterface
{
    private array $values = [];

    public static function createKey(string $key): ContextKeyInterface
    {
        return Context::createKey($key);
    }

    public static function getCurrent(): ContextInterface
    {
        return new self();
    }

    public function activate(): ScopeInterface
    {
        return Context::getRoot()->activate();
    }

    public function with(ContextKeyInterface $key, $value): ContextInterface
    {
        $next = clone $this;
        $next->values[spl_object_id($key)] = $value;
        return $next;
    }

    public function withContextValue(ImplicitContextKeyedInterface $value): ContextInterface
    {
        return $value->storeInContext($this);
    }

    public function get(ContextKeyInterface $key)
    {
        return $this->values[spl_object_id($key)] ?? null;
    }
}

final class UpperSetter implements PropagationSetterInterface
{
    public function set(&$carrier, string $key, string $value): void
    {
        $carrier[strtoupper($key)] = $value;
    }
}

final class UpperGetter implements PropagationGetterInterface
{
    public function keys($carrier): array
    {
        return array_keys($carrier);
    }

    public function get($carrier, string $key): ?string
    {
        return $carrier[strtoupper($key)] ?? null;
    }
}

final class RecordingBuilder implements BaggageBuilderInterface
{
    public array $entries = [];

    public function set(string $key, mixed $value, ?MetadataInterface $metadata = null): BaggageBuilderInterface
    {
        $this->entries[$key] = [$value, $metadata?->getValue()];
        return $this;
    }

    public function remove(string $key): BaggageBuilderInterface
    {
        unset($this->entries[$key]);
        return $this;
    }

    public function build(): BaggageInterface
    {
        return Baggage::getEmpty();
    }
}

$empty = Baggage::getEmpty();
check('empty-singletons',
    $empty === Baggage::getEmpty()
    && $empty === Baggage::fromContext(Context::getRoot())
    && $empty->isEmpty()
    && Metadata::getEmpty() === Metadata::getEmpty()
    && Metadata::getEmpty()->getValue() === ''
);

$builder = Baggage::getBuilder();
$same = $builder
    ->set('', 'ignored')
    ->set('tenant', 'acme', new Metadata('prop=1'))
    ->set('count', 42)
    ->set('nullable', null)
    ->set('message', 'hello world!~', new Metadata('0'));
$baggage = $builder->build();
$all = iterator_to_array($baggage->getAll());
check('builder-values',
    $same === $builder
    && array_keys($all) === ['tenant', 'count', 'nullable', 'message']
    && $baggage->getValue('tenant') === 'acme'
    && $baggage->getValue('count') === 42
    && $baggage->getEntry('nullable') instanceof Entry
    && $baggage->getValue('missing') === null
    && $baggage->getEntry('count')->getMetadata() === Metadata::getEmpty()
);

$changed = $baggage->toBuilder()->set('tenant', 'other')->remove('count')->build();
check('immutable-builder',
    $baggage->getValue('tenant') === 'acme'
    && $baggage->getValue('count') === 42
    && $changed->getValue('tenant') === 'other'
    && $changed->getEntry('count') === null
);

$cloneBuilder = clone $builder;
$cloneBuilder->set('clone-only', true);
check('clone-semantics',
    (clone $baggage)->getValue('tenant') === 'acme'
    && (clone $baggage->getEntry('tenant'))->getValue() === 'acme'
    && (clone new Metadata('x'))->getValue() === 'x'
    && $builder->build()->getEntry('clone-only') === null
    && $cloneBuilder->build()->getValue('clone-only') === true
);

$directEntry = new Entry('direct-value', Metadata::getEmpty());
$direct = new Baggage(['direct' => $directEntry]);
check('constructors', $direct->getEntry('direct') === $directEntry && $direct->getValue('direct') === 'direct-value');

$nativeContext = $baggage->storeInContext(Context::getRoot());
$userContext = $baggage->storeInContext(new UserContext());
check('context-store',
    Baggage::fromContext($nativeContext) === $baggage
    && Baggage::fromContext($userContext) === $baggage
    && $userContext instanceof UserContext
);
$scope = $baggage->activate();
check('context-activate', Baggage::getCurrent() === $baggage);
$scope->detach();
check('context-detach', Baggage::getCurrent() === Baggage::getEmpty());

$propagator = BaggagePropagator::getInstance();
check('propagator-shape',
    $propagator === BaggagePropagator::getInstance()
    && $propagator->fields() === ['baggage']
    && (clone $propagator)->fields() === ['baggage']
);
$carrier = [];
$propagator->inject($carrier, null, $userContext);
check('inject-default',
    $carrier['baggage'] === 'tenant=acme;prop=1,count=42,nullable=,message=hello+world%21%7E'
);
$upperCarrier = [];
$propagator->inject($upperCarrier, new UpperSetter(), $userContext);
check('inject-userland-setter', isset($upperCarrier['BAGGAGE']) && !isset($upperCarrier['baggage']));

$arrayObject = new ArrayObject(['Baggage' => 'stale=value']);
$propagator->inject($arrayObject, null, $userContext);
check('inject-array-access',
    count($arrayObject) === 1
    && $arrayObject['baggage'] === $carrier['baggage']
    && !isset($arrayObject['Baggage'])
);

$extracted = $propagator->extract($upperCarrier, new UpperGetter(), new UserContext());
$roundTrip = Baggage::fromContext($extracted);
check('extract-userland-getter',
    $extracted instanceof UserContext
    && $roundTrip->getValue('tenant') === 'acme'
    && $roundTrip->getEntry('tenant')->getMetadata()->getValue() === 'prop=1'
    && $roundTrip->getValue('count') === '42'
    && $roundTrip->getEntry('nullable') === null
    && $roundTrip->getEntry('message') === null
);
$unchanged = new UserContext();
check('extract-missing', $propagator->extract([], null, $unchanged) === $unchanged);

$parsedBuilder = Baggage::getBuilder();
(new Parser('good=hello%21;meta=1,plus=a%2Bb,slash=a%2Fb,bad key=x,zero=0,space=x%20y,key%2Fbad=x'))->parseInto($parsedBuilder);
$parsed = $parsedBuilder->build();
check('parser-validation',
    $parsed->getValue('good') === 'hello!'
    && $parsed->getEntry('good')->getMetadata()->getValue() === 'meta=1'
    && $parsed->getValue('plus') === 'a+b'
    && $parsed->getValue('slash') === 'a/b'
    && count(iterator_to_array($parsed->getAll())) === 3
);

$recording = new RecordingBuilder();
(new Parser('first=value;meta=yes,invalid,second=other'))->parseInto($recording);
check('parser-userland-builder',
    $recording->entries === [
        'first' => ['value', 'meta=yes'],
        'second' => ['other', null],
    ]
);

$limitedBuilder = Baggage::getBuilder()->set('existing', 'kept');
(new Parser(str_repeat('x', 8193)))->parseInto($limitedBuilder);
$members = ['invalid'];
for ($i = 0; $i < 185; $i++) {
    $members[] = 'key' . $i . '=value' . $i;
}
(new Parser(implode(',', $members)))->parseInto($limitedBuilder);
$limited = $limitedBuilder->build();
check('parser-limits',
    $limited->getValue('existing') === 'kept'
    && $limited->getValue('key179') === 'value179'
    && $limited->getEntry('key180') === null
    && count(iterator_to_array($limited->getAll())) === 181
);
?>
--EXPECT--
empty-singletons:ok
builder-values:ok
immutable-builder:ok
clone-semantics:ok
constructors:ok
context-store:ok
context-activate:ok
context-detach:ok
propagator-shape:ok
inject-default:ok
inject-userland-setter:ok
inject-array-access:ok
extract-userland-getter:ok
extract-missing:ok
parser-validation:ok
parser-userland-builder:ok
parser-limits:ok
