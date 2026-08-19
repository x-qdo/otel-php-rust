--TEST--
Disabled logs return no-op logger and builder objects
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=none
OTEL_LOGS_EXPORTER=none
OTEL_METRICS_EXPORTER=none
--INI--
otel.cli.enabled=1
otel.auto.enabled=0
--FILE--
<?php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Logs\MemoryLogsExporter;

$logger = Globals::loggerProvider()->getLogger('disabled.logger');
$builder = $logger->logRecordBuilder()->setBody('dropped');
var_dump($logger->isEnabled());
$builder->emit();
var_dump(MemoryLogsExporter::count());
?>
--EXPECT--
bool(false)
int(0)
