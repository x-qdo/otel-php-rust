--TEST--
Native logs support builders, events, severity enum, exceptions, and iterable attributes
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=none
OTEL_LOGS_EXPORTER=memory
OTEL_LOGS_PROCESSOR=simple
OTEL_METRICS_EXPORTER=none
--INI--
otel.cli.enabled=1
otel.auto.enabled=0
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Logs\EventLoggerProviderInterface;
use OpenTelemetry\API\Logs\LogRecordBuilderInterface;
use OpenTelemetry\API\Logs\MemoryLogsExporter;
use OpenTelemetry\API\Logs\Severity;

MemoryLogsExporter::reset();
$provider = Globals::loggerProvider();
$logger = $provider->getLogger('builder.logger');
$builder = $logger->logRecordBuilder();

var_dump(
    $provider instanceof EventLoggerProviderInterface,
    $builder instanceof LogRecordBuilderInterface,
    $logger->isEnabled(),
    Severity::WARN->value,
    Severity::fromPsr3('notice') === Severity::INFO2,
);
try {
    Severity::fromPsr3('unknown');
} catch (ValueError) {
    echo "value-error\n";
}

$attributes = (static function (): iterable {
    yield 'route' => '/checkout';
})();

$builder
    ->setTimestamp(1700000000000000000)
    ->setObservedTimestamp(1700000001000000000)
    ->setContext(false)
    ->setSeverityNumber(Severity::WARN)
    ->setSeverityText('warning-custom')
    ->setBody('builder-body')
    ->setAttributes($attributes)
    ->setException(new RuntimeException('boom'))
    ->setEventName('builder.event')
    ->emit();

$provider->getEventLogger('event.logger')->emit(
    name: 'event.api',
    body: 'event-body',
    severityNumber: Severity::INFO,
    attributes: ['source' => 'event'],
);

$logs = MemoryLogsExporter::getLogs();
echo count($logs), "\n";
echo $logs[0]['severity_number'], ':', $logs[0]['severity_text'], ':', $logs[0]['event_name'], "\n";
echo $logs[0]['attributes']['route'], ':', $logs[0]['attributes']['exception.message'], "\n";
var_dump(isset($logs[0]['trace_id']));
echo $logs[1]['severity_number'], ':', $logs[1]['event_name'], ':', $logs[1]['attributes']['source'], "\n";
?>
--EXPECT--
bool(true)
bool(true)
bool(true)
int(13)
bool(true)
value-error
2
13:warning-custom:builder.event
String(Owned("/checkout")):String(Owned("boom"))
bool(false)
9:event.api:String(Owned("event"))
