#!/usr/bin/env bash
# Syscall audit of the request thread under load against healthy, delayed and rejecting
# collectors. strace records every network/poll syscall per thread; the PHP main thread (tid ==
# pid) must never connect, send, receive, resolve or poll, while the exporter worker/runtime
# threads must show the collector traffic. Span::end() latency and the drain invariant are
# asserted on the same runs.

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/otlp_transport_lib.sh"
transport_test_init syscall-capture

spans="${OTEL_AUDIT_SPANS:-20000}"
start_services collector otlp-fixture blackhole collector-auth

# Syscalls a request thread must never issue while the exporter owns the network.
forbidden='^(socket|connect|sendto|sendmsg|sendmmsg|recvfrom|recvmsg|recvmmsg|poll|ppoll|epoll_wait|epoll_pwait|epoll_pwait2|select|pselect6)\('

# run_audit <case> <protocol> <endpoint> [-e VAR=value ...] : prints the JSON result line.
run_audit() {
    local case="$1" protocol="$2" endpoint="$3"
    shift 3
    mkdir -p "${capture_dir}/${case}"
    chmod 0777 "${capture_dir}/${case}"
    compose run --rm -T \
        -v "${capture_dir}:/capture" \
        -e "AUDIT_CASE=${case}" \
        -e "OTEL_AUDIT_SPANS=${spans}" \
        -e "OTEL_SERVICE_NAME=syscall-${case}" \
        -e "OTEL_EXPORTER_OTLP_PROTOCOL=${protocol}" \
        -e "OTEL_EXPORTER_OTLP_ENDPOINT=${endpoint}" \
        -e 'OTEL_TRACES_SAMPLER=always_on' \
        -e 'OTEL_LOGS_EXPORTER=none' \
        -e 'OTEL_BSP_SCHEDULE_DELAY=200' \
        -e 'OTEL_EXPORTER_OTLP_TIMEOUT=1000' \
        -e 'OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS=2' \
        -e 'OTEL_PHP_EXPORT_RETRY_INITIAL_BACKOFF=20' \
        -e 'OTEL_PHP_SHUTDOWN_TIMEOUT=500' \
        "$@" \
        php timeout 120 strace -f -ff -qq -o "/capture/${case}/trace" \
            -e trace=%network,poll,ppoll,epoll_wait,epoll_pwait,select,pselect6 \
            php -d extension=/usr/src/myapp/modules/otel.so -d otel.cli.enabled=1 \
                -d otel.log.level=error -d otel.log.file=/dev/stderr \
                tests/integration/request_thread_syscalls.php \
        2>"${capture_dir}/${case}.stderr" | tail -n 1
}

# assert_thread_isolation <case> <json> : main thread clean, some other thread did the network.
assert_thread_isolation() {
    local case="$1" json="$2" pid main_file others hits
    pid="$(jq -r '.pid' <<<"${json}")"
    main_file="${capture_dir}/${case}/trace.${pid}"
    if [[ ! -f "${main_file}" ]]; then
        echo "FAIL [${case}]: no strace file for main thread ${pid}" >&2
        ls "${capture_dir}/${case}" >&2
        return 1
    fi
    hits="$(grep -cE "${forbidden}" "${main_file}" || true)"
    if [[ "${hits}" -ne 0 ]]; then
        echo "FAIL [${case}]: main thread ${pid} issued ${hits} network/poll syscalls:" >&2
        grep -E "${forbidden}" "${main_file}" | head -n 20 | sed 's/^/    /' >&2
        return 1
    fi
    others="$(ls "${capture_dir}/${case}"/trace.* | grep -v "trace.${pid}$" || true)"
    if [[ -z "${others}" ]]; then
        echo "FAIL [${case}]: no exporter threads were traced" >&2
        return 1
    fi
    # shellcheck disable=SC2086
    if ! grep -lE '^(connect|sendmsg|sendto)\(' ${others} >/dev/null; then
        echo "FAIL [${case}]: no worker thread connected/sent to the collector" >&2
        return 1
    fi
    echo "ok [${case}]: main thread ${pid} clean; $(echo "${others}" | wc -l | tr -d ' ') worker thread trace(s) carry the network calls"
}

invariant='.metrics | .sampled_ended == .exported + .dropped_queue_full + .dropped_export_failure + .dropped_shutdown'
latency='.span_end_p99_ms < 1 and .span_end_max_ms < 50'

# Healthy collector, both transports: everything exports or is dropped only by the queue bound.
for transport in "grpc 4317" "http/protobuf 4318"; do
    set -- ${transport}
    case="healthy-${1%%/*}"
    result="$(run_audit "${case}" "$1" "http://collector:$2")"
    assert_thread_isolation "${case}" "${result}"
    assert_probe "${case}" "${result}" "${invariant}"
    assert_probe "${case}" "${result}" "${latency}"
    assert_probe "${case}" "${result}" '.metrics.exported > 0 and .metrics.export_failures == 0'
done

# Delayed collector: HTTP fixture holds every request 1.5 s (over the 1 s exporter timeout);
# gRPC blackhole accepts the connection and never answers. Exports time out, retry once, drop.
case="delayed-http"
result="$(run_audit "${case}" http/protobuf 'http://otlp-fixture:4318/mode/delay-1500')"
assert_thread_isolation "${case}" "${result}"
assert_probe "${case}" "${result}" "${invariant}"
assert_probe "${case}" "${result}" "${latency}"
assert_probe "${case}" "${result}" '.metrics.export_failures > 0 and .metrics.export_retries > 0 and .metrics.exported == 0'

case="delayed-grpc"
result="$(run_audit "${case}" grpc 'http://blackhole:4317')"
assert_thread_isolation "${case}" "${result}"
assert_probe "${case}" "${result}" "${invariant}"
assert_probe "${case}" "${result}" "${latency}"
assert_probe "${case}" "${result}" '.metrics.export_failures > 0 and .metrics.exported == 0'

# Rejecting collector: HTTP 503 (retryable, retried once then dropped) and gRPC
# UNAUTHENTICATED from the bearer-protected collector (terminal, one attempt).
case="rejecting-http"
result="$(run_audit "${case}" http/protobuf 'http://otlp-fixture:4318/mode/status-503')"
assert_thread_isolation "${case}" "${result}"
assert_probe "${case}" "${result}" "${invariant}"
assert_probe "${case}" "${result}" "${latency}"
assert_probe "${case}" "${result}" '.metrics.export_failures > 0 and .metrics.export_retries == .metrics.export_failures and .metrics.exported == 0'

case="rejecting-grpc"
result="$(run_audit "${case}" grpc 'http://collector-auth:4317')"
assert_thread_isolation "${case}" "${result}"
assert_probe "${case}" "${result}" "${invariant}"
assert_probe "${case}" "${result}" "${latency}"
assert_probe "${case}" "${result}" '.metrics.export_failures > 0 and .metrics.export_retries == 0 and .metrics.exported == 0'

echo "test_request_thread_syscalls: all assertions passed"
