#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
php_version="${PHP_VERSION:-8.2}"

run_transport() {
    local protocol="$1"

    PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" run --rm \
        -e 'OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:1' \
        -e "OTEL_EXPORTER_OTLP_PROTOCOL=${protocol}" \
        -e 'OTEL_EXPORTER_OTLP_TIMEOUT=200' \
        -e 'OTEL_BSP_MAX_QUEUE_SIZE=8' \
        -e 'OTEL_BSP_MAX_EXPORT_BATCH_SIZE=8' \
        -e 'OTEL_BSP_SCHEDULE_DELAY=60000' \
        -e 'OTEL_LOGS_EXPORTER=none' \
        php php \
        -d extension=/usr/src/myapp/modules/otel.so \
        -d otel.cli.enabled=1 \
        tests/integration/batch_export_failure.php
}

run_transport grpc
run_transport http/protobuf
