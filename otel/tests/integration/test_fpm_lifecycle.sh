#!/usr/bin/env bash
# PHP-FPM lifecycle in the release (Alpine/musl) image: requests across worker recycling
# (pm.max_requests), a USR2 reload, graceful (QUIT) and forced (TERM) termination. Every
# drained request's spans must reach the collector, forced-loss remains bounded, no worker
# may crash, and both terminations must complete within a bounded time.

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/otlp_transport_lib.sh"
transport_test_init fpm-capture

requests="${FPM_REQUESTS:-60}"
if ! docker image inspect otel-php-rust:php82-alpine-local >/dev/null 2>&1; then
    bash "${repo_root}/otel/tests/integration/test_alpine_php82_build.sh"
fi
start_services collector

result="$(compose run --rm -T \
    -e "FPM_REQUESTS=${requests}" \
    -e "FPM_OTEL_LOG_LEVEL=${FPM_OTEL_LOG_LEVEL:-warn}" \
    -e OTEL_SERVICE_NAME=fpm-lifecycle \
    -e OTEL_EXPORTER_OTLP_ENDPOINT=http://collector:4317 \
    -e OTEL_EXPORTER_OTLP_PROTOCOL=grpc \
    -e OTEL_BSP_SCHEDULE_DELAY=200 \
    -e OTEL_PHP_SHUTDOWN_TIMEOUT=500 \
    -e OTEL_LOGS_EXPORTER=none \
    php-fpm-alpine /srv/fpm_lifecycle.sh 2>"${capture_dir}/fpm.stderr" | tail -n 1)"
echo "${result}"

check() {
    if ! jq -e "$1" <<<"${result}" >/dev/null; then
        echo "FAIL: $1" >&2
        echo "  result: ${result}" >&2
        sed 's/^/    /' "${capture_dir}/fpm.stderr" | tail -n 60 >&2
        exit 1
    fi
    echo "ok: $1"
}

check ".phase1_ok == ${requests} and .phase2_ok == ${requests} and .phase3_ok == ${requests}"
check '.bad_responses == 0 and .crashes == 0 and .leftover_processes == 0'
# pm.max_requests=25 with 60 requests per phase plus the reload and restart recycle workers.
check '.distinct_worker_pids >= 6'
check '.graceful == "exit=0" and .graceful_ms < 5000'
check '.forced != "timeout" and .forced_ms < 3000'
# Worker diagnostics: no panic, no export failure.
if grep -qE 'internal panic|ExportError' "${capture_dir}/fpm.stderr"; then
    echo "FAIL: worker diagnostics contain panics or export errors" >&2
    grep -E 'internal panic|ExportError' "${capture_dir}/fpm.stderr" | head >&2
    exit 1
fi
echo "ok: no panic or export-failure diagnostics"

# Every request produced the auto HTTP root span plus the manual root and child span. The
# first two phases receive a scheduled-export drain window before reload/graceful shutdown;
# the forced shutdown may drop what was still queued from phase 3.
named_span_count() {
    jq -s --arg name "$1" '[ .[] | .resourceSpans[] | .scopeSpans[] | .spans[] | select(.name == $name) ] | length' \
        "${capture_dir}/traces.json" 2>/dev/null || echo 0
}
for _ in $(seq 1 30); do
    if [[ "$(named_span_count fpm-request)" -ge $(( requests * 2 )) ]]; then break; fi
    sleep 1
done
roots="$(named_span_count fpm-request)"; children="$(named_span_count fpm-work)"; auto="$(named_span_count GET)"
if [[ "${roots}" -lt $(( requests * 2 )) || "${roots}" -gt $(( requests * 3 )) || "${children}" -ne "${roots}" || "${auto}" -ne "${roots}" ]]; then
    echo "FAIL: collector has ${roots} fpm-request, ${children} fpm-work and ${auto} auto GET spans (expected ${requests}*2 <= n <= ${requests}*3, all equal)" >&2
    exit 1
fi
echo "ok: collector received ${roots} requests' spans (root, child and auto HTTP span each); forced shutdown dropped $(( requests * 3 - roots )) requests' spans"

echo "test_fpm_lifecycle: all assertions passed"
