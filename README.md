# OpenTelemetry + Rust

## Intro

This is a PHP extension built with [phper](https://github.com/phper-framework/phper)
that exposes [opentelemetry-rust](https://opentelemetry.io/docs/languages/rust/)
through the standard OpenTelemetry PHP API. It provides native traces, metrics,
logs, context, baggage, propagation, and selected auto-instrumentation.

This fork is optimized for allocation-lean manual instrumentation and bounded
background export in PHP-FPM, command, and long-running Messenger workers. Its
runtime guarantees, verification evidence, and remaining production-readiness work
are documented in [the production-readiness contract](docs/PRODUCTION_READINESS.md).

This repository is an independently maintained fork of
[Brett McBride's original `otel-php-rust` project](https://github.com/brettmc/otel-php-rust).
We gratefully acknowledge Brett McBride for creating the project and its original
implementation. The fork is maintained on its own roadmap; acceptance of its changes
upstream is not assumed.

## Supported PHP versions

The extension API baseline is PHP 8.1+, but CI and release artifacts follow PHP's
upstream-supported branches. The current mandatory matrix is PHP `8.2` through
`8.5`; PHP 8.0 and 8.1 are end-of-life and intentionally excluded. Every supported
minor receives separate glibc and musl builds for amd64 and arm64. The inherited
upstream PHP 7 code is not part of this fork's support or release matrix.

## Installation

Source builds require Rust and Cargo plus libclang development files (libclang 9.0
or newer; for example `libclang-dev` on Debian or `clang-dev` on Alpine).

### Manual

```shell
git clone <this repository>
cd otel-php-rust
git checkout <version>
make build
make install
```

### Prebuilt libraries

Tagged releases currently publish 16 dynamically linked libraries using this
naming scheme:

```text
otel-php8.<minor>-<x86_64|aarch64>-linux-<glibc|musl>.so
```

Select the exact PHP minor, CPU architecture, and libc used by the target image.
PHP extension module ABI is minor-specific, and glibc and musl libraries are not
interchangeable. Each library is accompanied by a uniquely named `.abi.json`
record containing the exact PHP version, Zend module ABI, NTS/debug flags,
architecture, libc family/version, linkage, and load-tested runtime image. Release
metadata also pins the runtime image digest. Assets are checksummed and their `.so`
files receive GitHub build-provenance attestations. Musl artifacts target Alpine
3.23; glibc artifacts target Debian Bookworm.

### Allocator

The extension links against the system allocator by default. An opt-in
[mimalloc](https://github.com/microsoft/mimalloc) build is available:

```shell
make build ALLOCATOR=mimalloc            # or: cargo build --release --features mimalloc
docker build -f Dockerfile.alpine --build-arg ALLOCATOR=mimalloc .
```

`php --ri otel` reports the compiled allocator (`allocator => system|mimalloc`).

Choose by sampling rate. In the same-machine, same-container microbenchmark
(`otel/tests/integration/benchmark_manual_spans.php`, PHP 8.2, gRPC to a local
Collector, medians of three interleaved runs):

| mode | system | mimalloc |
|---|---:|---:|
| disabled | 281 ns/op | 278 ns/op |
| parent-based 1% | 428 ns/op | 412 ns/op |
| always-on 100% burst | 1,010 ns/op | 777 ns/op |
| peak RSS (100% burst) | 27.8 MiB | 29.2 MiB |

At typical (≤ a few percent) sampling the allocator makes no measurable difference,
because a non-recording span allocates almost nothing. Under an always-on export
burst the exporter thread frees what the request thread allocated, and mimalloc's
thread-local heaps avoid the glibc cross-thread free slow path (about 20–25% less
request-thread time) for roughly 1–2 MiB more RSS per PHP process. Pick `mimalloc`
only if you run at high sampling rates and have measured the RSS budget for your
FPM pool size.

## Debugging

There's a bunch of logging from the extension and the underlying opentelemetry-rust and dependencies. It's configurable via `.ini`:

`otel.log.level` -> `error` (default), `warn`, `info`, `debug` or `trace`

`otel.log.file` -> `/dev/stderr` (default), or another file/location of your choosing

If you really want to see what's going on, set the log level to `trace` and you'll get a lot of logs.

## SAPI support

Production verification covers PHP-FPM and CLI, including command and long-running
Messenger-style workloads. CI also exercises PHP's built-in CLI server. The inherited
Apache and CGI paths are not part of the production approval matrix.

### `cli`
Does not auto-create a root span by default, use .ini `otel.cli.create_root_span` to enable.

Long-running CLI lifecycle and request-boundary cleanup are covered explicitly.

## Features

- Native OpenTelemetry PHP APIs for trace, metrics, logs, context, baggage,
  propagation, `Globals`, and `Signals`.
- Signature compatibility is reflection-gated against `open-telemetry/api` 1.10.0
  and `open-telemetry/context` 1.5.0. The policy has zero pending symbols. Every
  official Metrics and Logs interface is a native signature match; SDK utility,
  late-binding, and no-op implementation classes remain Composer-provided where
  the policy marks them `userland_only`.
- Full manual span lifecycle, context activation/storage, W3C trace context and
  baggage propagation, events, links, exceptions, limits, and local-root access.
- Synchronous metrics: counters, up/down counters, gauges, and histograms.
  Observable counters, up/down counters, and gauges support callback registration,
  batch observation, and detach.
- Logs: records, builders, events, severity enum, exception attributes,
  instrumentation scope, and automatic active-span correlation.
- `none`, `memory`, `console`, and OTLP exporters. OTLP metrics and logs work over
  both gRPC and HTTP/protobuf, as do traces.
- Bounded background processing for network export; trace request paths never wait
  on collector I/O. Simple processing is restricted to memory/console test and
  debug exporters.
- PID- and configuration-scoped providers for fork and long-worker safety.
- Auto-instrumentation of userland and internal code through the Zend Observer API,
  including Laravel, Laminas, Symfony, Zend Framework 1, and PSR-18 plugins.
- HTTP root spans, `traceparent` extraction, response status, and URL exclusions via
  `OTEL_PHP_EXCLUDED_URLS=/health*,/ping`.
- Shared-hosting configuration through per-application `.env` files and selective
  plugin disabling, for example `otel.auto.disabled_plugins=laminas,psr18`.

### Signal support

| Signal | Native API | Exporters | Production posture |
|---|---|---|---|
| Traces | Providers, tracers, builders, spans, context and propagation | none, memory, console, OTLP gRPC/HTTP | Configurable sampling, including parent-based 1% and 100% |
| Metrics | All official API interfaces; synchronous and observable instruments | none, memory, console, OTLP gRPC/HTTP | Export supported; validate volume and cost for each deployment |
| Logs | All official API interfaces; records, builders and events | none, memory, console, OTLP gRPC/HTTP | Export supported; validate volume and cost for each deployment |

Sampling percentage is deployment policy, not an OTLP exporter limit. To sample every
trace rooted in this service while preserving an upstream unsampled decision, use:

```shell
OTEL_TRACES_EXPORTER=otlp
OTEL_TRACES_SAMPLER=parentbased_always_on
```

This is 100% sampling, not an unconditional delivery guarantee. Export remains
bounded and non-blocking: saturation, collector failure, or a forced shutdown can
drop spans, with every drop reported by `TracerProvider::getRuntimeMetrics()`.

Metrics and logs are implemented and export successfully. Each deployment should
choose signal exporters and sampling based on its own telemetry volume, collector
capacity, retention policy, and cost budget.

## Configuration

### Native runtime

The native runtime is optimized for manual instrumentation. Network export uses a bounded,
non-blocking handoff to one background batch worker; requesting a Simple span processor
with an OTLP exporter is ignored. Runtime queue/export/drop counters are available from
`TracerProvider::getRuntimeMetrics()`.

Production adoption should use a deployment-specific benchmark, a staged canary, and
a tested rollback. See [the production-readiness contract](docs/PRODUCTION_READINESS.md)
for supported APIs, batch defaults and bounds, verification evidence, and remaining
generic production gates.

The reproducible comparison against the Composer-locked official PHP SDK is documented
in [the PHP SDK comparison](docs/evidence/2026-08-19-opentelemetry-php-comparison.md).
Run it with:

```shell
otel/tests/integration/run_php_sdk_comparison.sh
otel/tests/integration/run_php_sdk_blackhole_comparison.sh
```

### .ini
| Name                       | Default        | Description |
|----------------------------|----------------| ----------- |
| otel.log.level             | error          | Log level: error, warn, info, debug, trace |
| otel.log.file              | /dev/stderr    | Log destination: file or stdout/stderr |
| otel.cli.create_root_span  | false          | Whether to create a root span for CLI requests |
| otel.cli.enabled           | false          | Whether to enable OpenTelemetry for CLI requests |
| otel.env.set_from_server | false | Whether to set OTEL_* environment variables into the environment |
| otel.env.dotenv.enabled    | false          | Whether to load .env files per request |
| otel.auto.enabled          | true | Auto-instrumentation enabled |
| otel.auto.disabled_plugins | _empty string_ | A list of auto-instrumentation plugins to disable, comma-separated |

If either `otel.env.set_from_server` or `otel.env.dotenv.enabled` is set to true, the extension will back up the current
environment variables on RINIT, and restore them on RSHUTDOWN.

### Environment variables

Durations are in milliseconds. A signal-specific OTLP variable takes precedence over
its generic counterpart. `memory` and `console` exporters are intended for tests and
debugging. Only variables that affect the extension at runtime are listed;
test-harness variables are omitted.

#### General and signal configuration

| Variable | Default | Meaning |
|---|---|---|
| `OTEL_SERVICE_NAME` | `unknown_service` | Sets the `service.name` resource attribute. |
| `OTEL_RESOURCE_ATTRIBUTES` | _empty_ | Comma-separated resource attributes, for example `service.version=1.2.3,deployment.environment.name=production`. |
| `OTEL_SDK_DISABLED` | `false` | `true` returns no-op providers for the request. Values are case-insensitive. |
| `OTEL_PHP_EXCLUDED_URLS` | _empty_ | Comma-separated request URIs to exclude from instrumentation; `*` is a wildcard. |
| `OTEL_PHP_CAPTURE_SENSITIVE_DATA` | `false` | `true` allows automatic instrumentation to capture raw SQL, full URLs, exception messages, and stack traces. Treat this as a debug-only setting. |
| `OTEL_TRACES_EXPORTER` | `otlp` | Trace exporter: `none`, `memory`, `console`, or `otlp`. |
| `OTEL_METRICS_EXPORTER` | `otlp` | Metrics exporter: `none`, `memory`, `console`, or `otlp`. |
| `OTEL_LOGS_EXPORTER` | `otlp` | Logs exporter: `none`, `memory`, `console`, or `otlp`. |
| `OTEL_TRACES_SAMPLER` | `parentbased_always_on` | Sampler: `always_on`, `always_off`, `traceidratio`, `parentbased_always_on`, `parentbased_always_off`, or `parentbased_traceidratio`. |
| `OTEL_TRACES_SAMPLER_ARG` | `1.0` | Sampling probability from `0.0` to `1.0` for either ratio-based sampler. |
| `OTEL_SPAN_PROCESSOR` | `batch` | `simple` is honored only with `memory` and `console`; network trace export always uses bounded batching. |
| `OTEL_LOGS_PROCESSOR` | `batch` | `simple` is honored only with `memory` and `console`; network log export uses batching. |

#### OTLP exporter configuration

| Variable | Default | Meaning |
|---|---|---|
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `grpc` | Generic protocol: `grpc` or `http/protobuf`. |
| `OTEL_EXPORTER_OTLP_TRACES_PROTOCOL`<br>`OTEL_EXPORTER_OTLP_METRICS_PROTOCOL`<br>`OTEL_EXPORTER_OTLP_LOGS_PROTOCOL` | value of generic variable | Protocol override for the named signal. |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` for gRPC; `http://localhost:4318` for HTTP | Generic collector endpoint. OTLP/HTTP appends `/v1/traces`, `/v1/metrics`, or `/v1/logs`. |
| `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`<br>`OTEL_EXPORTER_OTLP_METRICS_ENDPOINT`<br>`OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` | value of generic variable | Full endpoint for the named signal; OTLP/HTTP does not append a signal path. |
| `OTEL_EXPORTER_OTLP_TIMEOUT` | traces: `3000`; metrics/logs: `10000` | Generic export timeout. Trace configuration is restricted to `1..30000`. |
| `OTEL_EXPORTER_OTLP_TRACES_TIMEOUT`<br>`OTEL_EXPORTER_OTLP_METRICS_TIMEOUT`<br>`OTEL_EXPORTER_OTLP_LOGS_TIMEOUT` | value of generic variable | Export-timeout override for the named signal. |
| `OTEL_EXPORTER_OTLP_HEADERS` | _empty_ | Generic comma-separated `key=value` request headers; percent-encode reserved characters. |
| `OTEL_EXPORTER_OTLP_TRACES_HEADERS`<br>`OTEL_EXPORTER_OTLP_METRICS_HEADERS`<br>`OTEL_EXPORTER_OTLP_LOGS_HEADERS` | value of generic variable | Request-header override for the named signal. |
| `OTEL_EXPORTER_OTLP_COMPRESSION` | _unset (no compression)_ | Generic compression. `gzip` is supported for every signal; the trace exporter also accepts an explicit `none`. |
| `OTEL_EXPORTER_OTLP_TRACES_COMPRESSION`<br>`OTEL_EXPORTER_OTLP_METRICS_COMPRESSION`<br>`OTEL_EXPORTER_OTLP_LOGS_COMPRESSION` | value of generic variable | Compression override for the named signal: `gzip` for every signal, or `none` for traces. |
| `OTEL_EXPORTER_OTLP_CERTIFICATE` / `OTEL_EXPORTER_OTLP_TRACES_CERTIFICATE` | bundled web PKI roots | Trace exporter only: path to a PEM CA bundle used instead of the bundled roots. |
| `OTEL_EXPORTER_OTLP_CLIENT_KEY` / `OTEL_EXPORTER_OTLP_TRACES_CLIENT_KEY` | _empty_ | Trace exporter only: path to a PEM client private key; must be set with the client certificate. |
| `OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE` / `OTEL_EXPORTER_OTLP_TRACES_CLIENT_CERTIFICATE` | _empty_ | Trace exporter only: path to a PEM client certificate; must be set with the client key. |
| `OTEL_EXPORTER_OTLP_INSECURE` / `OTEL_EXPORTER_OTLP_TRACES_INSECURE` | `false` | Trace exporter only: `true` makes a scheme-less gRPC `host:port` endpoint plaintext. It never downgrades an explicit `https://` endpoint. |
| `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` | process defaults | Standard proxy settings used by OTLP/HTTP. The gRPC transport connects directly. |

#### Batch, retry, and lifecycle configuration

| Variable | Default | Meaning |
|---|---:|---|
| `OTEL_BSP_MAX_QUEUE_SIZE` | `2048` | Maximum queued spans; valid range `1..65536`. |
| `OTEL_BSP_MAX_EXPORT_BATCH_SIZE` | `512` | Maximum spans per export; valid range `1..4096` and no greater than the queue size. |
| `OTEL_BSP_SCHEDULE_DELAY` | `1000` | Maximum delay before a partial trace batch is exported; valid range `1..60000`. |
| `OTEL_BSP_MAX_CONCURRENT_EXPORTS` | `1` | Trace batches allowed in flight; valid range `1..8`. Only gRPC overlaps exports. |
| `OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS` | `3` | Total trace export attempts; valid range `1..10`. `1` disables retries. |
| `OTEL_PHP_EXPORT_RETRY_MAX_ELAPSED` | `5000` | Per-batch trace retry budget; valid range `0..30000`. `0` disables retries. |
| `OTEL_PHP_EXPORT_RETRY_INITIAL_BACKOFF` | `100` | Initial trace retry backoff; valid range `1..5000`, then exponential backoff with jitter. |
| `OTEL_PHP_SHUTDOWN_TIMEOUT` | traces: `500`; logs: `5000` | Graceful provider shutdown budget. Traces accept `1..2000`; logs clamp to `1..60000`. |
| `OTEL_BLRP_MAX_QUEUE_SIZE` | `2048` | Maximum queued log records. |
| `OTEL_BLRP_MAX_EXPORT_BATCH_SIZE` | `512` | Maximum log records per batch; capped at the log queue size. |
| `OTEL_BLRP_SCHEDULE_DELAY` | `1000` | Maximum delay before a partial log batch is exported. |
| `OTEL_METRIC_EXPORT_INTERVAL` | `60000` | Interval between periodic metric collections and exports. |

#### Attribute limits

All configured limits below are non-negative integers capped at `1000000`.
An empty or invalid value uses the listed fallback. Metric attributes are exempt
because truncating them would change time-series identity.

| Variable | Default | Meaning |
|---|---:|---|
| `OTEL_ATTRIBUTE_COUNT_LIMIT` | `128` | General maximum number of attributes. |
| `OTEL_ATTRIBUTE_VALUE_LENGTH_LIMIT` | _unlimited_ | General maximum string length in Unicode characters. |
| `OTEL_SPAN_ATTRIBUTE_COUNT_LIMIT` | general count limit | Maximum attributes on a span. |
| `OTEL_SPAN_ATTRIBUTE_VALUE_LENGTH_LIMIT` | general value limit | Maximum string length for span, event, and link attributes. |
| `OTEL_EVENT_ATTRIBUTE_COUNT_LIMIT` | general count limit | Maximum attributes on each span event. |
| `OTEL_LINK_ATTRIBUTE_COUNT_LIMIT` | general count limit | Maximum attributes on each span link. |
| `OTEL_LOGRECORD_ATTRIBUTE_COUNT_LIMIT` | general count limit | Maximum attributes on a log record. |
| `OTEL_LOGRECORD_ATTRIBUTE_VALUE_LENGTH_LIMIT` | general value limit | Maximum string length for log-record attributes. |
| `OTEL_SPAN_EVENT_COUNT_LIMIT` | `128` | Maximum events retained by a span. |
| `OTEL_SPAN_LINK_COUNT_LIMIT` | `128` | Maximum links retained by a span. |
| `OTEL_PHP_ATTRIBUTE_KEY_LENGTH_LIMIT` | `256` | Maximum attribute-key length in Unicode characters; empty or overlong keys are dropped. |
| `OTEL_PHP_ATTRIBUTE_ARRAY_LENGTH_LIMIT` | `128` | Maximum number of elements retained in an attribute array. |

See [the production-readiness contract](docs/PRODUCTION_READINESS.md) for trace
transport validation, enforced bounds, retry behavior, and security details. Do not
assume that an arbitrary SDK environment variable is supported merely because it exists
in another OpenTelemetry language SDK.

If variables are not set in the process environment (eg via apache [SetEnv](https://httpd.apache.org/docs/current/env.html)),
then set `otel.env.set_from_server` via php.ini, and `OTEL_*` variables from `$_SERVER` will be set in the environment.

### .env files

`OTEL_SERVICE_NAME`, `OTEL_RESOURCE_ATTRIBUTES` and `OTEL_SDK_DISABLED` can be set in a `.env` file. Other variables should be
set in the environment (todo: could be relaxed to allow setting all OpenTelemetry SDK configuration variables in the `.env` file).

## Usage

### Auto-instrumentation

By installing the extension and providing the basic [SDK configuration](https://github.com/open-telemetry/opentelemetry-specification/blob/v1.46.0/specification/configuration/sdk-environment-variables.md#general-sdk-configuration)
that opentelemetry expects, each HTTP request will generate an HTTP server root span. There are some initial
auto-instrumentation plugins for some legacy frameworks.

### Manual instrumentation

#### Tracing

Basic usage:
```php
$provider = \OpenTelemetry\API\Globals::tracerProvider();
$tracer = $provider->getTracer('name', '0.1' /*other params*/);
$span = $tracer
    ->spanBuilder('test-span')
    ->setAttribute('key', 'value')
    ->startSpan();
$span->updateName('updated');
var_dump($span->getContext()->getTraceId());
$span
    ->setStatus('Ok')
    ->end();
```

Some more advanced stuff:
```php
$tracer = \OpenTelemetry\API\Globals::tracerProvider()->getTracer('my-tracer');
$root = $tracer->spanBuilder('root')->startSpan();
$scope = $root->activate();

//somewhere else in code
\OpenTelemetry\API\Trace\Span::getLocalRoot()->updateName('updated');

$root->end();
$scope->detach();
```

#### Metrics

```php
use OpenTelemetry\API\Globals;

$meter = Globals::meterProvider()->getMeter('checkout', '1.0');

$requests = $meter->createCounter('checkout.requests', '1', 'Checkout requests');
$requests->add(1, ['route' => '/checkout']);

$latency = $meter->createHistogram('checkout.duration', 'ms');
$latency->record(12.5, ['status' => 'ok']);

$queueDepth = $meter->createObservableGauge('checkout.queue.depth');
$registration = $queueDepth->observe(
    static fn ($observer) => $observer->observe(7, ['worker' => 'payments']),
);

// Keep the registration alive. Later, when it should no longer be observed:
// $registration->detach();
```

#### Logs

```php
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Logs\LogRecord;

$logger = Globals::loggerProvider()->getLogger("my_logger", '0.1');
$record = new LogRecord('hello otel');
$record
    ->setSeverityNumber(9) //info
    ->setSeverityText('Info')
    ->setEventName('my_event')
    ->setTimestamp((int) (microtime(true) * 1e9))
    ->setAttributes([
        'a_bool' => true,
        'an_int' => 1,
        'a_float' => 1.1,
        'a_string' => 'foo',
        'string_array' => ['one', 'two', 'three'],
        'int_array' => [1, 2, 3],
        'float_array' => [1.1, 2.2, 3.3],
        'bool_array' => [true, false, true],
    ]);
$logger->emit($record);
```

Note that if there is an active span when the log record is emitted, the span context
will be associated with the log record.

## Plugins

Mostly the framework plugins hook in to routing mechanism of that framework, update
the root span's name to something more meaningful, and add some attributes.

A couple of non-standard attributes are added if the data is available:
`php.framework.name`, `php.framework.module.name`, `php.framework.controller.name`,
`php.framework.action.name`, and `console.command`.

### Laminas

Hooks `Laminas\Mvc\MvcEvent::setRouteMatch`. Sets framework name, and uses the `RouteMatch` to set
module, controller and action names.
Hooks some of the `Laminas\Db` methods to create CLIENT spans for database queries.

### Laravel

Hooks `Illuminate\Contracts\Http\Kernel::handle`. Sets the framework name, updates
the request span name from Laravel's route URI template, and adds `http.route`,
controller, and action attributes when routing information is available.

Hooks `Illuminate\Console\Command::execute` to create an INTERNAL span for each
Artisan command. Non-zero exit codes and thrown exceptions mark the span as an
error. Disable the plugin with `otel.auto.disabled_plugins=laravel`.

### Symfony

Hooks `Symfony\Component\HttpKernel\HttpKernel::handle`. Sets the framework name,
updates the request span name from Symfony's resolved route name, and adds
`http.route`, controller, and action attributes when routing information is
available.

Hooks `Symfony\Component\Console\Command\Command::run` to create an INTERNAL span
for each console command. Non-zero exit codes and thrown exceptions mark the span
as an error. Laravel commands are excluded from this generic hook to prevent
duplicate spans. Disable the plugin with `otel.auto.disabled_plugins=symfony`.

### Zend Framework 1

Hooks `Zend_Controller_Router_Interface::route`. Sets framework name, and uses the
`Zend_Controller_Request_Abstract` to set module, controller and action names.

Hooks some Zend_Db methods to create CLIENT spans for database queries.

### Psr-18

Hooks `Psr\Http\Client\ClientInterface::sendRequest`, creates a CLIENT span and
injects the `traceparent` header into outgoing HTTP requests.

## Multi-site support

### Vhosts

If providing configuration via Apache `SetEnv` directives, or FPM `env[OTEL_*]` variables, you should
enable the `otel.env.set_from_server` setting in your php.ini, so that the extension
will set the environment variables from the server environment on RINIT, and restore them on RSHUTDOWN.

For example (untested):
```
<VirtualHost *:80>
    ServerName site1.example.com
    SetEnv OTEL_SERVICE_NAME my-service
    SetEnv OTEL_RESOURCE_ATTRIBUTES "service.namespace=site1,service.version=1.0"
    # Other config...
</VirtualHost>
```

If you cannot modify vhost config, you can also use the `.env` file support described below.

### No vhosts or multiple applications per vhost

If you have multiple sites on a single host (for example each application is a subdirectory of the web root), you can
use the `.env` file support to set the environment variables for each site.

During request startup (RINIT), the extension will look for a `.env` file in the directory of the
processed .php file (eg `/var/www/site1/public/index.php` -> `/var/www/site1/public/.env`),
and traverse up until `DOCUMENT_ROOT` is reached. If a .env file is found, it will be checked for
`OTEL_SDK_DISABLED`, `OTEL_SERVICE_NAME` and `OTEL_RESOURCE_ATTRIBUTES` variables, and if they are set,
they will be set in the current environment, and the original values restored at RSHUTDOWN.

NB that the  modified environment variables may not be reflected in `$_SERVER`, but should be visible via
`getenv()`.

### Opt-in or opt-out

OpenTelemetry can be either opt-in, or opt-out, using environment variables and `.env` files.

If you want to disable OpenTelemetry by default, and enable it for specific applications, you can set
`OTEL_SDK_DISABLED=true` in the server environment, and then set `OTEL_SDK_DISABLED=false` in the `.env` file
for each application you want to enable observability for.

If you want to enable OpenTelemetry by default, and disable it for specific applications, you can set
`OTEL_SDK_DISABLED=false` in the server environment, and then set `OTEL_SDK_DISABLED=true` in the `.env` file
for each application you want to disable observability for.

## Compatibility and production status

The old context-storage and missing-interface limitations no longer apply to this
fork. Compatibility is enforced by the reflection manifest in
`otel/tests/reflection/policy.json`; classes intentionally left to Composer are listed
there with a reason.

This fork is ready for controlled non-production evaluation, not blanket production
approval. Each adopter must provide production-shaped workload evidence and complete a
canary/rollback exercise; see
[Missing before production approval](docs/PRODUCTION_READINESS.md#missing-before-production-approval).
