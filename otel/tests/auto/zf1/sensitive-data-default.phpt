--TEST--
ZF1 automatic database spans omit raw SQL and exception details by default
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
OTEL_EXPORTER_OTLP_HEADERS=authorization=Bearer%20exporter-header-secret
HTTP_AUTHORIZATION=Bearer request-header-secret
--INI--
otel.log.level="warn"
otel.log.file="/dev/stdout"
otel.cli.enabled=1
--FILE--
<?php
require_once __DIR__ . '/vendor/autoload.php';

use OpenTelemetry\API\Trace\SpanExporter\Memory;

$dbname = __DIR__ . '/data/test.sqlite';
$db = new Zend_Db_Adapter_Pdo_Sqlite(['dbname' => $dbname]);
try {
    $stmt = $db->prepare("select * from does_not_exist where password = 'raw-sql-secret'");
    $stmt->execute();
} catch (Exception) {
}

$span = Memory::getSpans()[1];
$payload = json_encode(Memory::getSpans(), JSON_THROW_ON_ERROR);
$secrets = [
    'raw-sql-secret',
    'no such table',
    'request-header-secret',
    'exporter-header-secret',
];
var_dump($span['name']);
var_dump($span['status']);
var_dump(array_key_exists('db.query.text', $span['attributes']));
var_dump(array_keys($span['events'][0]['attributes']));
var_dump(array_filter($secrets, static fn (string $secret): bool => str_contains($payload, $secret)));
?>
--EXPECT--
string(29) "prepare SELECT does_not_exist"
string(25) "Error { description: "" }"
bool(false)
array(1) {
  [0]=>
  string(14) "exception.type"
}
array(0) {
}
