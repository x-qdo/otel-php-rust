--TEST--
PSR-18 automatic spans redact URL credentials, tokens, headers, and exception details by default
--EXTENSIONS--
otel
--SKIPIF--
<?php
if (PHP_VERSION_ID < 70200) {
    die('skip requires PHP 7.2+');
}
?>
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
use Nyholm\Psr7\Request;
use OpenTelemetry\API\Trace\SpanExporter\Memory;
use Psr\Http\Client\ClientExceptionInterface;
use Psr\Http\Client\ClientInterface;
use Psr\Http\Message\RequestInterface;
use Psr\Http\Message\ResponseInterface;

require __DIR__ . '/vendor/autoload.php';

class SensitiveClientException extends Exception implements ClientExceptionInterface {}
class SensitiveMockClient implements ClientInterface
{
    public function sendRequest(RequestInterface $request): ResponseInterface
    {
        throw new SensitiveClientException('exception-message-secret');
    }
}

$request = new Request(
    'GET',
    'http://url-user:url-password-secret@example.com/path?token=query-token-secret#fragment',
    ['Authorization' => 'Bearer psr-request-header-secret'],
);
try {
    (new SensitiveMockClient())->sendRequest($request);
} catch (ClientExceptionInterface) {
}

$span = Memory::getSpans()[0];
$payload = json_encode($span, JSON_THROW_ON_ERROR);
$secrets = [
    'url-user',
    'url-password-secret',
    'query-token-secret',
    'request-header-secret',
    'psr-request-header-secret',
    'exporter-header-secret',
    'exception-message-secret',
];
var_dump($span['attributes']['url.full']);
var_dump($span['status']);
var_dump(array_keys($span['events'][0]['attributes']));
var_dump(array_filter($secrets, static fn (string $secret): bool => str_contains($payload, $secret)));
?>
--EXPECT--
string(23) "http://example.com/path"
string(25) "Error { description: "" }"
array(1) {
  [0]=>
  string(14) "exception.type"
}
array(0) {
}
