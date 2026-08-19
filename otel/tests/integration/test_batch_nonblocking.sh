#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
php_version="${PHP_VERSION:-8.2}"

cleanup() {
    PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" \
        stop blackhole >/dev/null 2>&1 || true
}
trap cleanup EXIT

PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" \
    up -d --force-recreate blackhole

run_transport() {
    local protocol="$1"
    local port="$2"

    PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" run --rm \
        -e "OTEL_EXPORTER_OTLP_ENDPOINT=http://blackhole:${port}" \
        -e "OTEL_EXPORTER_OTLP_PROTOCOL=${protocol}" \
        -e 'OTEL_EXPORTER_OTLP_TIMEOUT=750' \
        -e 'OTEL_SPAN_PROCESSOR=simple' \
        -e 'OTEL_BSP_MAX_QUEUE_SIZE=8' \
        -e 'OTEL_BSP_MAX_EXPORT_BATCH_SIZE=4' \
        php php \
        -d extension=/usr/src/myapp/modules/otel.so \
        -d otel.cli.enabled=1 \
        tests/integration/span_end_nonblocking.php
}

run_transport grpc 4317
run_transport http/protobuf 4318
