#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
capture_dir="$(mktemp -d /tmp/otel-php-rust-capture.XXXXXX)"
chmod 0777 "${capture_dir}"
php_version="${PHP_VERSION:-8.2}"

cleanup() {
    OTEL_CAPTURE_DIR="${capture_dir}" PHP_VERSION="${php_version}" \
        docker compose --project-directory "${repo_root}" stop collector >/dev/null 2>&1 || true
    case "${capture_dir}" in
        /tmp/otel-php-rust-capture.*) rm -rf -- "${capture_dir}" ;;
    esac
}
trap cleanup EXIT

OTEL_CAPTURE_DIR="${capture_dir}" PHP_VERSION="${php_version}" \
    docker compose --project-directory "${repo_root}" up -d --force-recreate collector

for _ in $(seq 1 30); do
    if OTEL_CAPTURE_DIR="${capture_dir}" PHP_VERSION="${php_version}" \
        docker compose --project-directory "${repo_root}" logs --no-color collector \
        | grep -q 'Everything is ready'; then
        break
    fi
    sleep 1
done

run_transport() {
    local protocol="$1"
    local port="$2"
    local service="$3"

    PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" run --rm \
        -e "OTEL_EXPORTER_OTLP_ENDPOINT=http://collector:${port}" \
        -e "OTEL_EXPORTER_OTLP_PROTOCOL=${protocol}" \
        -e "OTEL_SERVICE_NAME=${service}" \
        -e 'OTEL_RESOURCE_ATTRIBUTES=deployment.environment.name=test,service.version=9.9.9' \
        -e 'OTEL_BSP_MAX_QUEUE_SIZE=16' \
        -e 'OTEL_BSP_MAX_EXPORT_BATCH_SIZE=3' \
        -e 'OTEL_BSP_SCHEDULE_DELAY=60000' \
        php php \
        -d extension=/usr/src/myapp/modules/otel.so \
        -d otel.cli.enabled=1 \
        tests/integration/otlp_trace_model.php
}

run_transport grpc 4317 conformance-grpc
run_transport http/protobuf 4318 conformance-http-protobuf

for _ in $(seq 1 30); do
    if [[ -f "${capture_dir}/traces.json" ]] && [[ "$(wc -l < "${capture_dir}/traces.json")" -ge 4 ]]; then
        break
    fi
    sleep 1
done

jq -s -f "${repo_root}/otel/tests/integration/assert_otlp_trace_model.jq" \
    "${capture_dir}/traces.json"
