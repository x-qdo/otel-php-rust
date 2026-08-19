#!/usr/bin/env bash
# OTLP transport conformance: oversize payloads against a 1 MiB collector limit, and collector
# partial-success / rejection responses from the OTLP/HTTP fixture. Every case asserts the
# runtime accounting, that the process stays healthy (a following small batch still exports)
# and that Span::end() never waited on the collector.

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/otlp_transport_lib.sh"
transport_test_init limits-capture

start_services collector-limits otlp-fixture

fixture_http='http://otlp-fixture:4318'
# One span carrying a 2 MiB attribute, then one small span: the first batch is rejected
# (gRPC RESOURCE_EXHAUSTED / HTTP 413), the second is delivered.
oversize='.rounds[0].metrics.export_failures == 1 and .rounds[0].metrics.dropped_export_failure == 1 and .rounds[0].metrics.exported == 0 and .rounds[1].metrics.exported == 1 and .rounds[1].metrics.export_failures == 1 and .rounds[0].max_span_end_ms < 50 and .rounds[1].max_span_end_ms < 5'

for transport in "grpc 4317" "http/protobuf 4318"; do
    set -- ${transport}
    case="oversize-${1%%/*}"
    result="$(PROBE_PLAN='1x2097152,1x0' run_probe "${case}" "$1" "http://collector-limits:$2" \
        -e 'OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS=1')"
    assert_probe "${case}" "${result}" "${oversize}"
    wait_collector_spans "transport-${case}" 1
done

# Oversize is RESOURCE_EXHAUSTED on gRPC (spec: retryable) and 413 on HTTP (terminal); with the
# default retry policy the gRPC batch is retried and still dropped, the HTTP batch is not.
case="oversize-grpc-retried"
result="$(PROBE_PLAN='1x2097152' run_probe "${case}" grpc 'http://collector-limits:4317' \
    -e 'OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS=2' -e 'OTEL_PHP_EXPORT_RETRY_INITIAL_BACKOFF=10')"
assert_probe "${case}" "${result}" '.rounds[0].metrics | .export_failures == 1 and .export_retries == 1 and .dropped_export_failure == 1'
case="oversize-http-terminal"
result="$(PROBE_PLAN='1x2097152' run_probe "${case}" http/protobuf 'http://collector-limits:4318' \
    -e 'OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS=2' -e 'OTEL_PHP_EXPORT_RETRY_INITIAL_BACKOFF=10')"
assert_probe "${case}" "${result}" '.rounds[0].metrics | .export_failures == 1 and .export_retries == 0 and .dropped_export_failure == 1'

# --- partial success -------------------------------------------------------------------------
# HTTP 200 with ExportTraceServicePartialSuccess{rejected_spans=3}: the batch counts as exported
# (the collector accepted the request) and the rejection is logged once per provider.
mark="$(fixture_mark)"
case="partial-success"
result="$(PROBE_PLAN='4x0,4x0' run_probe "${case}" http/protobuf "${fixture_http}/mode/partial-3")"
assert_probe "${case}" "${result}" '.rounds[1].metrics | .exported == 8 and .export_failures == 0 and .dropped_export_failure == 0'
assert_diagnostic "${case}" 'partial success: rejected_spans=3'
if [[ "$(grep -c 'partial success: rejected_spans=3' "${capture_dir}/probe-${case}.stderr")" -ne 1 ]]; then
    echo "FAIL [${case}]: partial-success diagnostic must be logged once per provider" >&2
    exit 1
fi
echo "ok [${case}]: partial-success diagnostic logged once"
records="$(fixture_records "${mark}" 'select(.role == "http")')"
assert_records "${case}" "${records}" 'length == 2 and all(.[]; .mode == "partial-3" and .status == 200)'

# A partial success with nothing rejected is silent.
case="partial-success-empty"
result="$(run_probe "${case}" http/protobuf "${fixture_http}/mode/partial-0")"
assert_probe "${case}" "${result}" '.rounds[0].metrics | .exported == 1'
assert_no_diagnostic "${case}" 'partial success'

# --- rejections ------------------------------------------------------------------------------
# Terminal statuses: one attempt, batch dropped, process healthy. Retryable statuses: retried up
# to the attempt budget (2 here), then dropped. Span::end() stays off the network either way.
for status in 400 401 404 413 500; do
    case="status-${status}"
    result="$(PROBE_PLAN='2x0,1x0' run_probe "${case}" http/protobuf "${fixture_http}/mode/status-${status}" \
        -e 'OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS=2' -e 'OTEL_PHP_EXPORT_RETRY_INITIAL_BACKOFF=10')"
    assert_probe "${case}" "${result}" '.rounds[1].metrics | .exported == 0 and .export_failures == 2 and .export_retries == 0 and .dropped_export_failure == 3 and .sampled_ended == 3'
    assert_probe "${case}" "${result}" '[.rounds[].max_span_end_ms] | max < 5'
done
for status in 429 502 503 504; do
    case="status-${status}"
    result="$(PROBE_PLAN='2x0,1x0' run_probe "${case}" http/protobuf "${fixture_http}/mode/status-${status}" \
        -e 'OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS=2' -e 'OTEL_PHP_EXPORT_RETRY_INITIAL_BACKOFF=10')"
    assert_probe "${case}" "${result}" '.rounds[1].metrics | .exported == 0 and .export_failures == 2 and .export_retries == 2 and .dropped_export_failure == 3 and .sampled_ended == 3'
    assert_probe "${case}" "${result}" '[.rounds[].max_span_end_ms] | max < 5'
done
# The drain invariant holds in every case above (sampled_ended == exported + dropped_*).
assert_probe "status-503" "${result}" '.rounds[1].metrics | .sampled_ended == .exported + .dropped_queue_full + .dropped_export_failure + .dropped_shutdown'

echo "test_otlp_transport_limits: all assertions passed"
