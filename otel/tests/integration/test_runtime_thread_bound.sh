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
        until nc -z collector-benchmark 4317; do
            attempt=$((attempt + 1))
            [ "$attempt" -lt 100 ] || exit 1
            sleep 0.1
        done
    '

PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" run --rm \
    -e OTEL_LOGS_EXPORTER=none \
    -e OTEL_EXPORTER_OTLP_ENDPOINT=http://collector-benchmark:4317 \
    -e OTEL_EXPORTER_OTLP_PROTOCOL=grpc \
    php php \
    -d extension=/usr/src/myapp/modules/otel.so \
    -d otel.cli.enabled=1 \
    tests/integration/runtime_thread_bound.php
