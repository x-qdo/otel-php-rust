#!/usr/bin/env bash
# OTLP transport conformance, wire level: request headers (valid, malformed, percent-encoded,
# duplicate, trace-specific precedence), compression precedence and invalid values, and proxy
# behaviour. The otlp-fixture service records every OTLP/HTTP request it receives, relays gRPC
# to the collector while logging each message's compression flag, and acts as an HTTP proxy.

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/otlp_transport_lib.sh"
transport_test_init wire-capture

start_services collector otlp-fixture

delivered='.rounds[0].metrics | .exported == 1 and .export_failures == 0'
noop='.rounds[0].metrics | .sampled_started == 0 and .sampled_ended == 0 and .queued == 0'
fixture_http='http://otlp-fixture:4318'
fixture_grpc='http://otlp-fixture:4317'

# --- headers -------------------------------------------------------------------------------
mark="$(fixture_mark)"
case="headers-http"
result="$(run_probe "${case}" http/protobuf "${fixture_http}" \
    -e 'OTEL_EXPORTER_OTLP_HEADERS=x-good=1, no-equals ,=novalue,x-empty=,x-enc=Bearer%20a%2Cb,X-Dup=first,x-dup=second,bad key=1')"
assert_probe "${case}" "${result}" "${delivered}"
records="$(fixture_records "${mark}" 'select(.role == "http")')"
assert_records "${case}" "${records}" 'length == 1 and .[0].headers["x-good"] == "1" and .[0].headers["x-enc"] == "Bearer a,b" and .[0].headers["x-dup"] == "second" and (.[0].headers | has("x-empty") | not) and (.[0].headers | has("no-equals") | not)'
assert_diagnostic "${case}" 'Ignoring malformed entry 2 of OTEL_EXPORTER_OTLP_HEADERS: missing'
assert_diagnostic "${case}" 'Ignoring malformed entry 3 of OTEL_EXPORTER_OTLP_HEADERS: empty header name'
assert_diagnostic "${case}" 'Ignoring malformed entry 4 of OTEL_EXPORTER_OTLP_HEADERS: empty header value'
assert_diagnostic "${case}" 'Ignoring malformed entry 8 of OTEL_EXPORTER_OTLP_HEADERS: invalid header name'
assert_diagnostic "${case}" 'Header x-dup is repeated'
# Header values never reach the diagnostics.
assert_no_diagnostic "${case}" 'Bearer'

mark="$(fixture_mark)"
case="headers-traces-precedence"
result="$(run_probe "${case}" http/protobuf "${fixture_http}" \
    -e 'OTEL_EXPORTER_OTLP_HEADERS=x-generic=1' \
    -e 'OTEL_EXPORTER_OTLP_TRACES_HEADERS=x-traces=1')"
assert_probe "${case}" "${result}" "${delivered}"
records="$(fixture_records "${mark}" 'select(.role == "http")')"
assert_records "${case}" "${records}" 'length == 1 and .[0].headers["x-traces"] == "1" and (.[0].headers | has("x-generic") | not)'

# gRPC headers are metadata on HTTP/2 and are proven end-to-end by test_otlp_auth_headers.sh
# (bearer-protected collector); here the same malformed list must not break the exporter.
case="headers-grpc-malformed-tolerated"
result="$(run_probe "${case}" grpc "${fixture_grpc}" \
    -e 'OTEL_EXPORTER_OTLP_HEADERS=x-good=1,no-equals,=novalue')"
assert_probe "${case}" "${result}" "${delivered}"
assert_diagnostic "${case}" 'Ignoring malformed entry 2 of OTEL_EXPORTER_OTLP_HEADERS'
wait_collector_spans "transport-${case}" 1

# --- compression ---------------------------------------------------------------------------
mark="$(fixture_mark)"
case="gzip-http"
result="$(run_probe "${case}" http/protobuf "${fixture_http}" -e 'OTEL_EXPORTER_OTLP_COMPRESSION=gzip')"
assert_probe "${case}" "${result}" "${delivered}"
records="$(fixture_records "${mark}" 'select(.role == "http")')"
assert_records "${case}" "${records}" 'length == 1 and .[0].body_gzip == true and .[0].headers["content-encoding"] == "gzip" and (.[0].headers["content-type"] | startswith("application/x-protobuf"))'

mark="$(fixture_mark)"
case="gzip-http-traces-none-wins"
result="$(run_probe "${case}" http/protobuf "${fixture_http}" \
    -e 'OTEL_EXPORTER_OTLP_COMPRESSION=gzip' -e 'OTEL_EXPORTER_OTLP_TRACES_COMPRESSION=none')"
assert_probe "${case}" "${result}" "${delivered}"
records="$(fixture_records "${mark}" 'select(.role == "http")')"
assert_records "${case}" "${records}" 'length == 1 and .[0].body_gzip == false and (.[0].headers | has("content-encoding") | not)'

mark="$(fixture_mark)"
case="gzip-http-traces-gzip-wins"
result="$(run_probe "${case}" http/protobuf "${fixture_http}" \
    -e 'OTEL_EXPORTER_OTLP_COMPRESSION=none' -e 'OTEL_EXPORTER_OTLP_TRACES_COMPRESSION=gzip')"
assert_probe "${case}" "${result}" "${delivered}"
records="$(fixture_records "${mark}" 'select(.role == "http")')"
assert_records "${case}" "${records}" 'length == 1 and .[0].body_gzip == true'

mark="$(fixture_mark)"
case="gzip-grpc"
result="$(run_probe "${case}" grpc "${fixture_grpc}" -e 'OTEL_EXPORTER_OTLP_COMPRESSION=gzip')"
assert_probe "${case}" "${result}" "${delivered}"
records="$(fixture_records "${mark}" 'select(.role == "relay" and .event == "grpc_message")')"
assert_records "${case}" "${records}" 'length >= 1 and all(.[]; .compressed_flag == 1 and .gzip_magic == true)'
wait_collector_spans "transport-${case}" 1

mark="$(fixture_mark)"
case="plain-grpc"
result="$(run_probe "${case}" grpc "${fixture_grpc}" -e 'OTEL_EXPORTER_OTLP_COMPRESSION=none')"
assert_probe "${case}" "${result}" "${delivered}"
records="$(fixture_records "${mark}" 'select(.role == "relay" and .event == "grpc_message")')"
assert_records "${case}" "${records}" 'length >= 1 and all(.[]; .compressed_flag == 0)'
wait_collector_spans "transport-${case}" 1

case="compression-invalid"
result="$(run_probe "${case}" http/protobuf "${fixture_http}" -e 'OTEL_EXPORTER_OTLP_COMPRESSION=brotli')"
assert_probe "${case}" "${result}" "${noop}"
assert_diagnostic "${case}" 'Unsupported OTLP compression'

# --- proxy ---------------------------------------------------------------------------------
# OTLP/HTTP honours HTTP_PROXY/HTTPS_PROXY/NO_PROXY (reqwest default): the request reaches the
# collector through the fixture proxy, which logs it.
mark="$(fixture_mark)"
case="proxy-http"
result="$(run_probe "${case}" http/protobuf 'http://collector:4318' -e 'HTTP_PROXY=http://otlp-fixture:3128')"
assert_probe "${case}" "${result}" "${delivered}"
records="$(fixture_records "${mark}" 'select(.role == "proxy")')"
assert_records "${case}" "${records}" 'length == 1 and .[0].method == "POST" and (.[0].target | startswith("http://collector:4318/v1/traces"))'
wait_collector_spans "transport-${case}" 1

mark="$(fixture_mark)"
case="proxy-http-no-proxy"
result="$(run_probe "${case}" http/protobuf 'http://collector:4318' \
    -e 'HTTP_PROXY=http://otlp-fixture:3128' -e 'NO_PROXY=collector')"
assert_probe "${case}" "${result}" "${delivered}"
records="$(fixture_records "${mark}" 'select(.role == "proxy")')"
assert_records "${case}" "${records}" 'length == 0'
wait_collector_spans "transport-${case}" 1

# A proxy that is unreachable is an export failure, never a request-thread wait.
case="proxy-http-unreachable"
result="$(run_probe "${case}" http/protobuf 'http://collector:4318' -e 'HTTP_PROXY=http://127.0.0.1:1')"
assert_probe "${case}" "${result}" '.rounds[0].metrics | .exported == 0 and .export_failures == 1 and .dropped_export_failure == 1'
assert_probe "${case}" "${result}" '.rounds[0].max_span_end_ms < 5'

# gRPC (tonic) does not use proxy environment variables: the export still goes direct.
mark="$(fixture_mark)"
case="proxy-grpc-ignored"
result="$(run_probe "${case}" grpc 'http://collector:4317' \
    -e 'HTTP_PROXY=http://otlp-fixture:3128' -e 'HTTPS_PROXY=http://otlp-fixture:3128')"
assert_probe "${case}" "${result}" "${delivered}"
records="$(fixture_records "${mark}" 'select(.role == "proxy")')"
assert_records "${case}" "${records}" 'length == 0'
wait_collector_spans "transport-${case}" 1

echo "test_otlp_transport_wire: all assertions passed"
