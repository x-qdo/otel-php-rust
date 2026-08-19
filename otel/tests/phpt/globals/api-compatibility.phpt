--TEST--
Globals and Signals provide cached native providers, composite propagation, and Composer overrides
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_LOGS_EXPORTER=memory
OTEL_METRICS_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
--INI--
otel.cli.enabled=1
otel.log.level=error
--FILE--
<?php
use OpenTelemetry\API\Baggage\Baggage;
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Signals;
use OpenTelemetry\API\Trace\Span;
use OpenTelemetry\API\Trace\SpanContext;
use OpenTelemetry\API\Trace\TraceFlags;
use OpenTelemetry\API\Trace\TraceState;
use OpenTelemetry\Context\Context;
use OpenTelemetry\Context\Propagation\MultiTextMapPropagator;
use OpenTelemetry\Context\Propagation\NativeNoopResponsePropagator;

function check(string $name, bool $condition): void
{
    echo $name, ':', $condition ? 'ok' : 'fail', "\n";
}

check('signals',
    Signals::TRACE === 'trace'
    && Signals::METRICS === 'metrics'
    && Signals::LOGS === 'logs'
);

Globals::reset();
$tracerProvider = Globals::tracerProvider();
$meterProvider = Globals::meterProvider();
$loggerProvider = Globals::loggerProvider();
$eventLoggerProvider = Globals::eventLoggerProvider();
$propagator = Globals::propagator();
$responsePropagator = Globals::responsePropagator();
check('cached-globals',
    $tracerProvider === Globals::tracerProvider()
    && $meterProvider === Globals::meterProvider()
    && $loggerProvider === Globals::loggerProvider()
    && $eventLoggerProvider === Globals::eventLoggerProvider()
    && $propagator === Globals::propagator()
    && $responsePropagator === Globals::responsePropagator()
    && $loggerProvider === $eventLoggerProvider
);
check('native-types',
    get_class($tracerProvider) === 'OpenTelemetry\\API\\Trace\\TracerProvider'
    && get_class($meterProvider) === 'OpenTelemetry\\API\\Metrics\\MeterProvider'
    && get_class($loggerProvider) === 'OpenTelemetry\\API\\Logs\\LoggerProvider'
    && $propagator instanceof MultiTextMapPropagator
    && $responsePropagator instanceof NativeNoopResponsePropagator
);
check('composite-fields', $propagator->fields() === ['traceparent', 'tracestate', 'baggage']);

$traceId = '2b4ef3412d587ce6e7880fb27a316b8c';
$spanId = '7480a670201f6340';
$spanContext = SpanContext::createFromRemoteParent(
    $traceId,
    $spanId,
    TraceFlags::SAMPLED,
    new TraceState('vendor=value'),
);
$context = Span::wrap($spanContext)->storeInContext(Context::getRoot());
$context = Baggage::getBuilder()
    ->set('tenant', 'acme')
    ->set('region', 'eu west')
    ->build()
    ->storeInContext($context);
$carrier = new ArrayObject(['TraceParent' => 'stale', 'Baggage' => 'stale=value']);
$propagator->inject($carrier, null, $context);
check('composite-inject',
    !isset($carrier['TraceParent'])
    && !isset($carrier['Baggage'])
    && $carrier['traceparent'] === '00-' . $traceId . '-' . $spanId . '-01'
    && $carrier['tracestate'] === 'vendor=value'
    && $carrier['baggage'] === 'tenant=acme,region=eu+west'
);

$incoming = new ArrayObject([
    'TRACEPARENT' => '00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01',
    'TRACESTATE' => 'upstream=value',
    'BAGGAGE' => 'customer=42,zone=north-one',
]);
$extracted = $propagator->extract($incoming, null, Context::getRoot());
check('composite-trace-extract',
    Span::fromContext($extracted)->getContext()->isRemote()
    && Span::fromContext($extracted)->getContext()->getTraceId() === 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
    && (string) Span::fromContext($extracted)->getContext()->getTraceState() === 'upstream=value'
);
check('composite-baggage-extract',
    Baggage::fromContext($extracted)->getValue('customer') === '42'
    && Baggage::fromContext($extracted)->getValue('zone') === 'north-one'
);

$responseCarrier = ['unchanged' => 'yes'];
$responsePropagator->inject($responseCarrier, null, $context);
check('response-noop', $responseCarrier === ['unchanged' => 'yes']);

$constructed = new Globals(
    $tracerProvider,
    $meterProvider,
    $loggerProvider,
    $eventLoggerProvider,
    $propagator,
    $responsePropagator,
);
check('constructor-clone', clone $constructed instanceof Globals);

require dirname(__DIR__, 2) . '/reflection/vendor/autoload.php';

$replacement = new MultiTextMapPropagator([]);
$initializerCalls = 0;
Globals::reset();
Globals::registerInitializer(
    static function (OpenTelemetry\API\Instrumentation\Configurator $configurator): OpenTelemetry\API\Instrumentation\Configurator {
        throw new RuntimeException('expected initializer failure');
    },
);
Globals::registerInitializer(
    static function (OpenTelemetry\API\Instrumentation\Configurator $configurator) use ($replacement, &$initializerCalls): OpenTelemetry\API\Instrumentation\Configurator {
        ++$initializerCalls;
        return $configurator->withPropagator($replacement);
    },
);
check('composer-initializer',
    Globals::propagator() === $replacement
    && Globals::propagator() === $replacement
    && $initializerCalls === 1
);

$baseTracer = Globals::tracerProvider();
$overrideTracer = new class($baseTracer) implements OpenTelemetry\API\Trace\TracerProviderInterface {
    public function __construct(private OpenTelemetry\API\Trace\TracerProviderInterface $delegate) {}

    public function getTracer(
        string $name,
        ?string $version = null,
        ?string $schemaUrl = null,
        iterable $attributes = [],
    ): OpenTelemetry\API\Trace\TracerInterface {
        return $this->delegate->getTracer($name, $version, $schemaUrl, $attributes);
    }
};
$overridePropagator = new MultiTextMapPropagator([]);
$overrideContext = Context::getRoot()
    ->with(OpenTelemetry\API\Instrumentation\ContextKeys::tracerProvider(), $overrideTracer)
    ->with(OpenTelemetry\API\Instrumentation\ContextKeys::propagator(), $overridePropagator);
$scope = $overrideContext->activate();
check('context-overrides',
    Globals::tracerProvider() === $overrideTracer
    && Globals::propagator() === $overridePropagator
);
$scope->detach();
check('context-restored', Globals::tracerProvider() === $baseTracer && Globals::propagator() === $replacement);

Globals::reset();
check('reset', Globals::propagator() !== $replacement);
?>
--EXPECT--
signals:ok
cached-globals:ok
native-types:ok
composite-fields:ok
composite-inject:ok
composite-trace-extract:ok
composite-baggage-extract:ok
response-noop:ok
constructor-clone:ok
composer-initializer:ok
context-overrides:ok
context-restored:ok
reset:ok
