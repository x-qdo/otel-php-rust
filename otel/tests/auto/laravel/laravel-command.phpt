--TEST--
Test Laravel command instrumentation
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=memory
OTEL_SPAN_PROCESSOR=simple
--INI--
otel.log.level="warn"
otel.cli.enabled=1
otel.cli.create_root_span=1
--FILE--
<?php
require __DIR__ . '/vendor/autoload.php';

use Illuminate\Console\Command;
use Illuminate\Foundation\Application;
use OpenTelemetry\API\Trace\SpanExporter\Memory;
use Symfony\Component\Console\Input\ArrayInput;
use Symfony\Component\Console\Output\BufferedOutput;

final class LaravelFixtureCommand extends Command
{
    protected $signature = 'fixture:laravel';

    public function handle(): int
    {
        return self::SUCCESS;
    }
}

final class LaravelFailingFixtureCommand extends Command
{
    protected $signature = 'fixture:laravel-fail';

    public function handle(): int
    {
        return self::FAILURE;
    }
}

$application = new Application();
$command = new LaravelFixtureCommand();
$command->setLaravel($application);
$exitCode = $command->run(new ArrayInput([]), new BufferedOutput());

$failingCommand = new LaravelFailingFixtureCommand();
$failingCommand->setLaravel($application);
$failingExitCode = $failingCommand->run(new ArrayInput([]), new BufferedOutput());

$spans = Memory::getSpans();
$span = $spans[0];
$failingSpan = $spans[1];

var_dump($exitCode);
var_dump($failingExitCode);
var_dump(count($spans));
var_dump($span['name']);
var_dump($span['span_kind']);
var_dump($span['instrumentation_scope']['name']);
var_dump($span['status']);
var_dump($span['attributes']['php.framework.name']);
var_dump($span['attributes']['console.command']);
var_dump(str_starts_with($failingSpan['status'], 'Error'));
var_dump($failingSpan['attributes']['console.command']);
?>
--EXPECT--
int(0)
int(1)
int(2)
string(23) "Command fixture:laravel"
string(8) "Internal"
string(21) "php.otel.auto.laravel"
string(5) "Unset"
string(7) "laravel"
string(15) "fixture:laravel"
bool(true)
string(20) "fixture:laravel-fail"
