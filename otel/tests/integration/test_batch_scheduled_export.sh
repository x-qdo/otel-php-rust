#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
php_version="${PHP_VERSION:-8.2}"

cleanup() {
    PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" \
        stop collector-benchmark >/dev/null 2>&1 || true
}
trap cleanup EXIT

PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" \
    up -d --force-recreate collector-benchmark

PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" run --rm -T \
    php sh -ec '
        attempt=0
        until nc -z collector-benchmark 4317 && nc -z collector-benchmark 4318; do
            attempt=$((attempt + 1))
            [ "$attempt" -lt 100 ] || exit 1
            sleep 0.1
        done
    '

run_transport() {
    local protocol="$1"
    local port="$2"
    local compression_variable="${3:-}"
    local compression_args=()
    if [[ -n "${compression_variable}" ]]; then
        compression_args=(-e "${compression_variable}=gzip")
    fi

    PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" run --rm \
        -e "OTEL_EXPORTER_OTLP_ENDPOINT=http://collector-benchmark:${port}" \
        -e "OTEL_EXPORTER_OTLP_PROTOCOL=${protocol}" \
        "${compression_args[@]}" \
        -e 'OTEL_BSP_MAX_QUEUE_SIZE=8' \
        -e 'OTEL_BSP_MAX_EXPORT_BATCH_SIZE=8' \
        -e 'OTEL_BSP_SCHEDULE_DELAY=25' \
        -e 'OTEL_LOGS_EXPORTER=none' \
        php php \
        -d extension=/usr/src/myapp/modules/otel.so \
        -d otel.cli.enabled=1 \
        tests/integration/batch_scheduled_export.php
}

run_transport grpc 4317
run_transport http/protobuf 4318
run_transport grpc 4317 OTEL_EXPORTER_OTLP_COMPRESSION
run_transport http/protobuf 4318 OTEL_EXPORTER_OTLP_TRACES_COMPRESSION
