# Production readiness contract

Status: implemented prototype; not production-approved

Upstream baseline: `brettmc/otel-php-rust` 0.17.0,
`b913449636d8e5a69ef07345ffd173479e8779f0`

Fork version: `0.18.0`

## Scope

This fork provides native OpenTelemetry PHP APIs with allocation-lean manual
instrumentation and bounded export. It must create, parent, mutate, propagate, batch,
and export manual spans without putting collector network waits on PHP request,
command, or message-processing threads.

Auto-instrumentation plugins require their own compatibility tests and benchmarks
before being presented as production-ready. The native runtime exposes trace,
metrics, logs, context, and baggage APIs; production approval remains gated by the
checklist below.

## Required runtime behavior

### Manual API and context

The native API must support the reflection-locked OpenTelemetry PHP API surface:

- tracer providers, instrumentation scope name/version/schema/attributes, tracers, and
  span builders;
- root and child spans, explicit/current/remote parents, span kind, explicit start
  timestamps, and links;
- string, integer, float, boolean, and homogeneous array attributes on builders,
  spans, events, links, and exception events;
- `isRecording()`, `getContext()`, `setAttribute(s)`, `addEvent()`, `addLink()`,
  `recordException()`, `setStatus()`, `updateName()`, `activate()`, `storeInContext()`,
  `fromContext()`, and idempotent `end()`;
- nested current context, safe in-order and out-of-order detach, cleanup after detach,
  remote/sampled flags, trace state, and W3C `traceparent`/`tracestate`/`baggage`
  extraction and injection, including the default composite global propagator.

The completed OTLP payload must preserve resource, instrumentation-scope, span, event,
link, status, exception, trace ID, parent span ID, and sampled-flag data in both gRPC
and HTTP/protobuf transports.

### Attribute validation and limits

Trace, event, link, log-record, and instrumentation-scope attributes enforce the
OpenTelemetry count and Unicode-character value limits. The general settings are
`OTEL_ATTRIBUTE_COUNT_LIMIT` (default 128) and
`OTEL_ATTRIBUTE_VALUE_LENGTH_LIMIT` (unset means no string limit). Trace settings can
override these with `OTEL_SPAN_ATTRIBUTE_COUNT_LIMIT` and
`OTEL_SPAN_ATTRIBUTE_VALUE_LENGTH_LIMIT`; event/link counts use
`OTEL_EVENT_ATTRIBUTE_COUNT_LIMIT` and `OTEL_LINK_ATTRIBUTE_COUNT_LIMIT`; log records
use `OTEL_LOGRECORD_ATTRIBUTE_COUNT_LIMIT` and
`OTEL_LOGRECORD_ATTRIBUTE_VALUE_LENGTH_LIMIT`. `OTEL_SPAN_EVENT_COUNT_LIMIT` and
`OTEL_SPAN_LINK_COUNT_LIMIT` bound the collections themselves. Empty values mean
unset, invalid values warn once and use the fallback, and accepted configured limits
are capped at 1,000,000 to prevent configuration-driven preallocation abuse.

Two extension-specific defensive limits cover inputs for which OpenTelemetry defines
no standard environment variable: `OTEL_PHP_ATTRIBUTE_KEY_LENGTH_LIMIT` defaults to
256 characters and drops overlong/empty keys without truncating them, while
`OTEL_PHP_ATTRIBUTE_ARRAY_LENGTH_LIMIT` defaults to 128 elements. String-array members
are truncated individually. Attribute arrays must contain only one primitive type;
integer/double mixtures are the one compatible numeric family, while nested, null,
object, resource, and other mixed arrays drop the whole attribute instead of silently
losing elements. Empty arrays remain valid. Log `AnyValue` lists/maps apply limits
recursively, but log bodies remain exempt. Metric attributes also remain exempt from
truncation and deletion, as required by the OpenTelemetry Metrics SDK specification,
because changing them changes time-series identity.

Automatic sensitive-data capture is disabled by default. Only the case-insensitive
value `true` for `OTEL_PHP_CAPTURE_SENSITIVE_DATA` enables it; unset, empty, and
`false` remain off, while any other value warns once and stays off. In safe mode,
database spans keep low-cardinality operation/table names but omit `db.query.text`,
and HTTP client/server `url.full` values remove userinfo, query, and fragment
components. Automatically observed exceptions keep only `exception.type` and an
empty error status description. Authorization/request/exporter headers are never
copied into span attributes. The opt-in restores raw SQL, full URLs, exception
messages, stack traces, and status descriptions and must therefore be treated as a
debug-only production exception. Explicit application calls to
`Span::recordException()` and log-builder exception APIs are not automatic capture
and preserve the official API behavior.

### Export isolation

Network exporters always use `BoundedBatchSpanProcessor`. A network configuration that
requests `OTEL_SPAN_PROCESSOR=simple` is forced to bounded batching. Simple processing
remains available only for the in-memory and console test/debug exporters.

`Span::end()` performs local finalization and one non-blocking bounded-channel attempt.
It does not perform DNS, connect, send, retry, flush, or collector-response waits. The
single exporter worker drains in batches and may keep up to
`OTEL_BSP_MAX_CONCURRENT_EXPORTS` batches in flight (only the async gRPC transport
overlaps requests; the blocking HTTP client serialises them). Raising it increases
drain throughput at the cost of exporter-side CPU that competes with PHP on a saturated
host, so the default stays 1; the gRPC transport additionally owns a one-worker Tokio
runtime. A full queue drops the newest span and
increments the exact drop counter.

Defaults and enforced bounds are:

| Setting | Default | Valid bound |
|---|---:|---:|
| `OTEL_BSP_MAX_QUEUE_SIZE` | 2,048 | 1..65,536 |
| `OTEL_BSP_MAX_EXPORT_BATCH_SIZE` | 512 | 1..4,096 and no greater than queue size |
| `OTEL_BSP_SCHEDULE_DELAY` | 1,000 ms | 1..60,000 ms |
| `OTEL_EXPORTER_OTLP_TRACES_TIMEOUT` / `OTEL_EXPORTER_OTLP_TIMEOUT` | 3,000 ms | 1..30,000 ms |
| `OTEL_PHP_SHUTDOWN_TIMEOUT` | 500 ms | 1..2,000 ms |
| `OTEL_BSP_MAX_CONCURRENT_EXPORTS` | 1 | 1..8 |
| `OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS` | 3 | 1..10 (1 disables retry) |
| `OTEL_PHP_EXPORT_RETRY_MAX_ELAPSED` | 5,000 ms | 0..30,000 ms |
| `OTEL_PHP_EXPORT_RETRY_INITIAL_BACKOFF` | 100 ms | 1..5,000 ms |

Invalid endpoint, protocol, batch, or retry configuration selects a no-op trace provider
and emits a bounded diagnostic. Export failure stays on the worker: a failed attempt is
classified by the OTLP retry guidance (retryable: gRPC `CANCELLED`, `DEADLINE_EXCEEDED`,
`ABORTED`, `OUT_OF_RANGE`, `UNAVAILABLE`, `DATA_LOSS`, `RESOURCE_EXHAUSTED`; HTTP 429,
502, 503, 504; connect/DNS/transport/timeout failures — everything else is terminal) and
a retryable batch is re-exported with exponential backoff (x2, ±20 % jitter, capped at
5 s) until `OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS` attempts or the per-batch
`OTEL_PHP_EXPORT_RETRY_MAX_ELAPSED` budget is spent; each attempt is still bounded by the
exporter timeout. Concurrent batches of one round back off together. The worker sleeps
in ≤10 ms slices during a backoff, so a `forceFlush()`/shutdown request cuts the backoff
short and the shutdown budget aborts remaining attempts. Terminal or exhausted batches are
dropped and accounted; the request thread never participates. Graceful shutdown waits no
longer than the configured shutdown budget, even if an exporter shutdown call is stuck.

### OTLP transport

Both `grpc` and `http/protobuf` are supported. Standard generic and trace-specific
endpoint, protocol, timeout, compression, header and TLS environment variables
participate in the provider configuration; the `OTEL_EXPORTER_OTLP_TRACES_*` form always
wins over the generic one and empty values count as unset. Bearer headers and gzip
exports work for both transports. Credentials, header values and provider configuration
hashes are never logged.

- Endpoint: `http://`/`https://` URLs without userinfo. A scheme-less gRPC endpoint must
  be `host:port` and defaults to TLS; `OTEL_EXPORTER_OTLP_INSECURE=true` selects
  plaintext for it and never downgrades an explicit `https://`. OTLP/HTTP always uses the
  URL scheme. Anything else is invalid and selects the no-op provider with a diagnostic.
- TLS (both transports, pure-Rust rustls, no OpenSSL in the musl image): server
  certificates are verified against the bundled webpki roots, or only against
  `OTEL_EXPORTER_OTLP_CERTIFICATE` when it is set; `OTEL_EXPORTER_OTLP_CLIENT_KEY` +
  `OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE` (both required together) enable mutual TLS.
  Unreadable or non-PEM files and half identities select the no-op provider with a
  diagnostic that names the variable, never a blind or unverified export.
- Headers: `OTEL_EXPORTER_OTLP_HEADERS` is `key=value,...` with percent-encoded values;
  a malformed entry (no `=`, empty name or value, invalid header name, bad encoding,
  non-ASCII) is skipped with a diagnostic that names only its position, the rest of the
  list is sent, and a repeated key keeps the last value. The provider is never downgraded
  for a malformed header. gRPC header delivery is proven end-to-end through a
  bearer-protected collector; HTTP header delivery is proven on the wire.
- Compression: `gzip` or `none`; the trace-specific variable overrides the generic one;
  any other value selects the no-op provider. Proven on the wire for HTTP
  (`Content-Encoding: gzip`, gzip body) and for gRPC (message compressed flag observed by
  a relay).
- Proxy: OTLP/HTTP honours `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY` (CONNECT or absolute
  URI through the proxy; an unreachable proxy is an accounted export failure). The gRPC
  transport (tonic) ignores proxy variables and always connects directly.
- Oversize payloads are rejected by the collector (gRPC `RESOURCE_EXHAUSTED`, HTTP 413):
  the batch is dropped and accounted (`RESOURCE_EXHAUSTED` is retried within the retry
  budget, 413 is terminal), the next batch exports normally and `Span::end()` stays
  sub-millisecond.
- Collector responses: HTTP 2xx with an `ExportTraceServicePartialSuccess` keeps the
  batch counted as exported and logs `rejected_spans`/message once per provider (silent
  when nothing is rejected); 400/401/404/413/500 are terminal (one attempt), 429/502/503/
  504 are retried up to the attempt budget, then dropped; the drain invariant holds.

### Disabled and no-op providers

When the SDK is disabled (`OTEL_SDK_DISABLED=true`, CLI without `otel.cli.enabled`) or a
provider cannot be built, `getTracer()` returns a stateless tracer: `spanBuilder()` copies
nothing, `startSpan()` returns a `NonRecordingSpan` with an invalid span context, and
`activate()`/`storeInContext()` install an invalid current span, matching the official
API's no-op behaviour. `OTEL_TRACES_EXPORTER=none` is not "disabled": it still samples
nothing but keeps valid IDs for propagation.

### Request-thread cost

The manual span path is allocation-lean by construction: PHP-visible methods resolve their
native handler through a reserved slot in `zend_internal_function` (no per-call name
lookup, see `otel/third_party/phper/UPSTREAM.md`), object creation resolves the native
class entry in O(1), builder setters mutate in place, span methods consult native state
before PHP properties, and instrumentation-scope names are interned so tracer clones do
not allocate. Attribute keys are interned in a bounded per-thread table (4,096 distinct
keys, then owned keys), so builder → span → export-data clones copy pointers. The
bounded channel carries boxed `SpanData`, keeping the ring buffer at pointer size.
Provider configuration is hashed once per request in RINIT. The global allocator is
selectable at build time (`ALLOCATOR=mimalloc`, see README "Allocator").

### Lifecycle and diagnostics

Providers and runtimes are keyed by PID and effective provider configuration. A child
process initializes its own exporter worker/runtime and does not reuse a parent thread
or socket. Context objects keep native context state through detach so a recovered span
can still be ended, while plain detached contexts are released. Long-running workers
must not retain per-span Zend handler allocations.

Panic strategy: the extension is built with `panic = "unwind"` and every engine entry
point that runs extension code (phper's method/function `invoke`, MINIT/MSHUTDOWN/RINIT/
RSHUTDOWN/MINFO hooks, object create/clone/free handlers, observer hooks) catches the
unwind. A panic inside a PHP-visible method surfaces as a catchable PHP `\Error`
(`otel: internal error: <message>`) with a null return value; a panic in a lifecycle hook
or observer is logged and the request continues (only a failed MINIT refuses to load the
module). A rate-limited process panic hook (first 10 with message and location, then one
suppression notice) writes to the extension log. PHP-reachable code must not use
`unwrap()`/`expect()`/`panic!`/`unreachable!`/slice indexing (`make lint` runs clippy with
those lints denied); missing native state (objects created without their constructor)
and missing arguments are reported as PHP errors instead.

`TracerProvider::getRuntimeMetrics()` returns:

- `sampled_started`, `sampled_ended`, `queued`, and `exported`;
- `dropped_queue_full`, `dropped_export_failure`, and `dropped_shutdown`;
- `export_failures` (terminal batch failures), `export_retries` (retry attempts
  performed), `export_retry_recovered` (batches exported after at least one retry),
  `queue_depth`, `queue_high_watermark`, and `in_flight`.

The accounting invariant after a completed drain is:

`sampled_ended = exported + dropped_queue_full + dropped_export_failure + dropped_shutdown`

for the observed provider lifetime. During an active export, `queue_depth` and
`in_flight` describe separate bounded sets.

## Implemented and verified in the fork

- bounded non-blocking batch handoff, scheduled partial-batch export, exact queue-full
  and export-failure accounting, rate-limited failure diagnostics, and bounded shutdown;
- a bounded transient retry policy on the exporter worker (OTLP retryable-status
  classification, attempt and elapsed budgets, jittered exponential backoff that yields to
  flush/shutdown) with `export_retries`/`export_retry_recovered` accounting, proven by
  unit tests against fake exporters and a connection-refused integration run for both
  transports where `Span::end()` stays sub-millisecond during retries;
- OTLP/gRPC and OTLP/HTTP-protobuf export to a real Collector, including headers,
  generic/trace-specific configuration, typed payloads, batching, and gzip smoke paths;
- transport conformance (`otel/tests/integration/test_otlp_transport_{tls,wire,limits}.sh`):
  TLS server verification, custom CA, mutual TLS, `INSECURE`/scheme-less endpoints and
  invalid TLS configuration against a TLS collector with per-run certificates; wire-level
  header sanitising and precedence, gzip precedence on both transports, HTTP proxy and
  `NO_PROXY` behaviour through a recording fixture; oversize payloads against a 1 MiB
  collector limit; partial-success and 4xx/5xx rejection accounting including retry
  classification;
- W3C trace-context round trips, parent-based/fixed-ID sampler behavior, manual span
  lifecycle, exception/status/event/link handling, and detached-context recovery;
- a request-thread syscall audit (`otel/tests/integration/test_request_thread_syscalls.sh`):
  20k sampled spans per case traced with `strace -ff` against healthy, delayed (HTTP fixture
  holding requests past the exporter timeout, gRPC blackhole) and rejecting (HTTP 503,
  gRPC `UNAUTHENTICATED`) collectors on both transports; the PHP main thread issues no
  socket/connect/send/recv/poll/select calls while worker threads carry all collector
  traffic, `Span::end()` stays p99 < 1 ms, bounded `forceFlush()` returns while the worker
  keeps retrying, and the drain invariant holds after the worker drains;
- PID-scoped provider/runtime behavior across a real PHP fork;
- a shared `phper` object-handler table fixing linear native RSS growth;
- panic containment at every FFI boundary with a rate-limited panic hook, an explicit
  `panic = "unwind"` strategy, and a clippy gate (`make lint`) that rejects
  `unwrap`/`expect`/`panic`/indexing on non-test code; phpts prove that panics in
  functions, methods, state constructors, RINIT/RSHUTDOWN and observer hooks leave the
  process and the span path working;
- PHP 8.2 FPM Alpine 3.18/aarch64-musl production-image build and clean extension
  load using dynamic musl linkage;
- PHP-FPM lifecycle coverage in the release Alpine image: requests survive
  `pm.max_requests` worker recycling and a master `USR2` reload, graceful `QUIT` and
  forced `TERM` terminate within bounded budgets, completed request spans export, and
  worker logs contain no crash/panic/export-failure diagnostics; a repeated 20k-unit
  Messenger-style extract/activate/nest/detach loop proves clean unit boundaries and
  bounded RSS; and an explicit linux/amd64 buildx test loads the x86_64-musl artifact in
  the matching PHP 8.2 FPM image and validates its ABI and recording path;
- a mandatory CI pipeline (`.github/workflows/ci.yaml`, no `continue-on-error`): Rust
  tests, the clippy panic-site gate and the phpt/HTTP/auto suites on every
  upstream-supported PHP 8 minor (currently 8.2 through 8.5) with
  the toolchain pinned by `otel/rust-toolchain.toml` (the same 1.97.1 the docker images
  install), cargo-deny advisories/licence/source policy (`otel/deny.toml`, rustls-only TLS
  enforced by banning OpenSSL crates), the docker-based verification matrix
  (`make integration`: manifest, export isolation, retry, OTLP model/auth, transport
  conformance, syscall audit, fork, long-worker and Messenger context/lifecycle, FPM
  reload/termination, and aarch64/x86_64 Alpine build/load), and for every
  build a 16-entry compatibility-artifact matrix for PHP 8.2 through 8.5, aarch64
  and x86_64, and both musl (`Dockerfile.alpine`) and glibc
  (`Dockerfile.glibc`). Every artifact is loaded in its matching official PHP FPM
  image (Alpine 3.23 for musl, Debian Bookworm for glibc) and has uniquely named
  ABI metadata (exact PHP version, Zend module ABI, NTS/debug flags, architecture,
  libc family/version, dynamic linkage, and runtime image tag/digest), checksums,
  and GitHub build provenance. Tags publish all libraries and metadata alongside a
  CycloneDX SBOM, `cargo about` licence inventory, and advisory report; and
- randomized five-run native microbenchmarks for no extension, loaded-disabled,
  parent-based 1%, and always-on gRPC/HTTP modes; and
- a two-process reflection manifest (`otel/tests/integration/test_reflection_manifest.sh`):
  one PHP process dumps the public surface of the Composer-locked `open-telemetry/api`
  1.10.0 and `open-telemetry/context` 1.5.0 packages, a second process dumps what the
  extension declares, and `compare_manifest.php` enforces `tests/reflection/policy.json`,
  where every official name is classified as `match` (identical signature required),
  `pending` (known gap, warning) or `userland_only` (must stay Composer-provided), and
  every extension-only name needs a reason; and
- a Composer-locked, same-operation comparison against official OpenTelemetry PHP SDK
  1.15.0 over gRPC and HTTP/protobuf, including a scheduled blackhole case. At 1%
  sampling the fork used about 16% of the official SDK's loop time (about 26% before
  the write-path optimizations) and its disabled path is faster than the official
  no-op provider; when export became
  due against the blackhole, native `Span::end()` stayed below 0.006 ms while the
  official SDK spent about 757-761 ms in the call path.

## Missing before production approval

These are release blockers or explicitly unproven areas. Plugin-specific framework
coverage is tracked separately from the native API and exporter contract.

Completed compatibility work:

- The reflection policy has zero `pending` names. All Composer-locked API/context names
  are either enforced native matches (with documented safe exceptions for two array
  constants) or explicitly userland-only, covering trace, context, baggage, logs,
  metrics, `Globals`, and `Signals`.
- W3C baggage is part of the native compatibility contract and the default global
  propagator composes trace context, trace state, and baggage.
- Attribute count/value/event/link limits, defensive key/array bounds, whole-array
  validation, and safe-by-default SQL/URL/header/token/exception capture are enforced
  and covered across trace, metrics, logs, scopes, ZF1, and PSR-18 paths.

Sampling policy is not a runtime ceiling. A full-sampling
profile uses `OTEL_TRACES_EXPORTER=otlp` and
`OTEL_TRACES_SAMPLER=parentbased_always_on`; this samples every trace rooted in the
service while preserving an upstream unsampled parent decision. It does not promise
lossless delivery: the bounded non-blocking exporter records queue-full, export-failure,
and shutdown drops in the runtime accounting described above. A production 100%
profile therefore still requires trace-volume, collector-capacity, and cost gates.

Remaining blockers:

1. Run production-shaped web, command, and long-worker benchmarks on fixed
   infrastructure. The native and official-SDK comparison microbenchmarks do not
   satisfy application-specific HTTP p95/p99, throughput, CPU/request, error-rate, or
   representative-workload gates. The evidence validator in `otel/tests/production`
   requires five randomized paired runs for every representative workload plus
   collector-fault, exporter-accounting, syscall, and 100,000-span longevity evidence.
   Applications, datasets, and replay traffic are deliberately deployment-owned inputs
   and are not included in this repository.
2. Deploy a release-attested artifact to a representative canary, verify exporter and
   application health gates, exercise `OTEL_SDK_DISABLED=true`, and rehearse rollback to
   the previous application image. This repository cannot provide production approval
   for an adopter's infrastructure or workload.

Until these items pass, the fork is suitable for continued development and controlled
non-production evaluation, not a blanket replacement for the current production path.
