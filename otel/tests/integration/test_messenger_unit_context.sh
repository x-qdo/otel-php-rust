#!/usr/bin/env bash
# Repeated Messenger-unit context: a long-running worker extracts, activates, nests,
# detaches and ends per unit of work; RSS stays flat across 20k units, every unit starts
# from a clean current context, and the sampled spans stream to the blackhole exporter
# (queue-bound drops are expected and accounted; nothing blocks the worker).

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
php_version="${PHP_VERSION:-8.2}"

cleanup() {
    PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" \
        stop blackhole >/dev/null 2>&1 || true
}
trap cleanup EXIT

PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" up -d --force-recreate blackhole >/dev/null

PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" run --rm \
    -e OTEL_TRACES_SAMPLER=always_on \
    -e OTEL_EXPORTER_OTLP_ENDPOINT=http://blackhole:4317 \
    -e OTEL_EXPORTER_OTLP_PROTOCOL=grpc \
    -e OTEL_EXPORTER_OTLP_TIMEOUT=500 \
    -e OTEL_BSP_SCHEDULE_DELAY=200 \
    -e OTEL_PHP_SHUTDOWN_TIMEOUT=500 \
    -e OTEL_LOGS_EXPORTER=none \
    php php \
    -d extension=/usr/src/myapp/modules/otel.so \
    -d otel.cli.enabled=1 \
    tests/integration/messenger_unit_context.php
