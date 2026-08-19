#!/usr/bin/env bash
# OTLP transport conformance: TLS server verification, custom CA, mTLS, INSECURE and
# scheme-less gRPC endpoints, invalid TLS configuration. Certificates are generated per run.

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/otlp_transport_lib.sh"
transport_test_init tls-capture

generate_test_pki
start_services collector-tls

delivered='.rounds[0].metrics | .exported == 1 and .export_failures == 0 and .dropped_export_failure == 0'
failed='.rounds[0].metrics | .exported == 0 and .export_failures == 1 and .dropped_export_failure == 1 and .sampled_ended == 1'
noop='.rounds[0].metrics | .sampled_started == 0 and .sampled_ended == 0 and .queued == 0'

# (a) https without a trusted CA: the server certificate is rejected, nothing is delivered.
for transport in "grpc 4317" "http/protobuf 4318"; do
    set -- ${transport}
    case="tls-untrusted-${1%%/*}"
    result="$(run_probe "${case}" "$1" "https://collector-tls:$2")"
    assert_probe "${case}" "${result}" "${failed}"
    assert_no_collector_spans "transport-${case}"
done

# (b) custom CA via OTEL_EXPORTER_OTLP_CERTIFICATE (generic) and the trace-specific override.
for transport in "grpc 4317" "http/protobuf 4318"; do
    set -- ${transport}
    case="tls-ca-${1%%/*}"
    result="$(run_probe "${case}" "$1" "https://collector-tls:$2" \
        -e OTEL_EXPORTER_OTLP_CERTIFICATE=/capture/ca.crt)"
    assert_probe "${case}" "${result}" "${delivered}"
    wait_collector_spans "transport-${case}" 1

    case="tls-traces-ca-${1%%/*}"
    result="$(run_probe "${case}" "$1" "https://collector-tls:$2" \
        -e OTEL_EXPORTER_OTLP_CERTIFICATE=/capture/other-ca.crt \
        -e OTEL_EXPORTER_OTLP_TRACES_CERTIFICATE=/capture/ca.crt)"
    assert_probe "${case}" "${result}" "${delivered}"
    wait_collector_spans "transport-${case}" 1
done

# (d) wrong CA: a CA that did not sign the server certificate fails verification.
for transport in "grpc 4317" "http/protobuf 4318"; do
    set -- ${transport}
    case="tls-wrong-ca-${1%%/*}"
    result="$(run_probe "${case}" "$1" "https://collector-tls:$2" \
        -e OTEL_EXPORTER_OTLP_CERTIFICATE=/capture/other-ca.crt)"
    assert_probe "${case}" "${result}" "${failed}"
    assert_no_collector_spans "transport-${case}"
done

# (c) mutual TLS: the collector requires a client certificate on 4319/4320.
for transport in "grpc 4319" "http/protobuf 4320"; do
    set -- ${transport}
    case="mtls-${1%%/*}"
    result="$(run_probe "${case}" "$1" "https://collector-tls:$2" \
        -e OTEL_EXPORTER_OTLP_CERTIFICATE=/capture/ca.crt \
        -e OTEL_EXPORTER_OTLP_CLIENT_KEY=/capture/client.key \
        -e OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE=/capture/client.crt)"
    assert_probe "${case}" "${result}" "${delivered}"
    wait_collector_spans "transport-${case}" 1

    case="mtls-traces-${1%%/*}"
    result="$(run_probe "${case}" "$1" "https://collector-tls:$2" \
        -e OTEL_EXPORTER_OTLP_CERTIFICATE=/capture/ca.crt \
        -e OTEL_EXPORTER_OTLP_CLIENT_KEY=/capture/server.key \
        -e OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE=/capture/server.crt \
        -e OTEL_EXPORTER_OTLP_TRACES_CLIENT_KEY=/capture/client.key \
        -e OTEL_EXPORTER_OTLP_TRACES_CLIENT_CERTIFICATE=/capture/client.crt)"
    assert_probe "${case}" "${result}" "${delivered}"
    wait_collector_spans "transport-${case}" 1

    case="mtls-missing-identity-${1%%/*}"
    result="$(run_probe "${case}" "$1" "https://collector-tls:$2" \
        -e OTEL_EXPORTER_OTLP_CERTIFICATE=/capture/ca.crt)"
    assert_probe "${case}" "${result}" "${failed}"
    assert_no_collector_spans "transport-${case}"
done

# Invalid TLS configuration selects the no-op provider with a diagnostic, never a blind export.
case="tls-missing-ca-file"
result="$(run_probe "${case}" grpc https://collector-tls:4317 \
    -e OTEL_EXPORTER_OTLP_CERTIFICATE=/capture/does-not-exist.crt)"
assert_probe "${case}" "${result}" "${noop}"
assert_diagnostic "${case}" 'OTEL_EXPORTER_OTLP_CERTIFICATE'
assert_diagnostic "${case}" 'no-op'

case="tls-invalid-ca-pem"
result="$(run_probe "${case}" http/protobuf https://collector-tls:4318 \
    -e OTEL_EXPORTER_OTLP_CERTIFICATE=/capture/invalid.pem)"
assert_probe "${case}" "${result}" "${noop}"
assert_diagnostic "${case}" 'OTEL_EXPORTER_OTLP_CERTIFICATE'

case="tls-half-identity"
result="$(run_probe "${case}" grpc https://collector-tls:4319 \
    -e OTEL_EXPORTER_OTLP_CERTIFICATE=/capture/ca.crt \
    -e OTEL_EXPORTER_OTLP_CLIENT_KEY=/capture/client.key)"
assert_probe "${case}" "${result}" "${noop}"
assert_diagnostic "${case}" 'OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE'

case="tls-invalid-client-key"
result="$(run_probe "${case}" http/protobuf https://collector-tls:4320 \
    -e OTEL_EXPORTER_OTLP_CERTIFICATE=/capture/ca.crt \
    -e OTEL_EXPORTER_OTLP_CLIENT_KEY=/capture/invalid.pem \
    -e OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE=/capture/client.crt)"
assert_probe "${case}" "${result}" "${noop}"
assert_diagnostic "${case}" 'OTEL_EXPORTER_OTLP_CLIENT_KEY'

# Scheme-less gRPC endpoints default to TLS; OTEL_EXPORTER_OTLP_INSECURE=true selects plaintext.
case="grpc-schemeless-secure"
result="$(run_probe "${case}" grpc collector-tls:4317 \
    -e OTEL_EXPORTER_OTLP_CERTIFICATE=/capture/ca.crt)"
assert_probe "${case}" "${result}" "${delivered}"
wait_collector_spans "transport-${case}" 1

case="grpc-schemeless-insecure"
result="$(run_probe "${case}" grpc collector-tls:4327 \
    -e OTEL_EXPORTER_OTLP_INSECURE=true)"
assert_probe "${case}" "${result}" "${delivered}"
wait_collector_spans "transport-${case}" 1

case="grpc-schemeless-traces-insecure"
result="$(run_probe "${case}" grpc collector-tls:4327 \
    -e OTEL_EXPORTER_OTLP_INSECURE=false \
    -e OTEL_EXPORTER_OTLP_TRACES_INSECURE=true)"
assert_probe "${case}" "${result}" "${delivered}"
wait_collector_spans "transport-${case}" 1

# INSECURE never downgrades an explicit https:// scheme.
case="grpc-https-insecure-ignored"
result="$(run_probe "${case}" grpc https://collector-tls:4317 \
    -e OTEL_EXPORTER_OTLP_INSECURE=true)"
assert_probe "${case}" "${result}" "${failed}"
assert_no_collector_spans "transport-${case}"

# OTLP/HTTP always uses the endpoint scheme: a scheme-less endpoint is invalid there.
case="http-schemeless-invalid"
result="$(run_probe "${case}" http/protobuf collector-tls:4328 \
    -e OTEL_EXPORTER_OTLP_INSECURE=true)"
assert_probe "${case}" "${result}" "${noop}"
assert_diagnostic "${case}" 'Invalid OTLP endpoint'

echo "test_otlp_transport_tls: all assertions passed"
