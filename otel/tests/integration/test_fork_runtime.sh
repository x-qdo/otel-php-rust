#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
capture_dir="$(mktemp -d /tmp/otel-php-rust-fork.XXXXXX)"
php_version="${PHP_VERSION:-8.2}"

cleanup() {
    OTEL_CAPTURE_DIR="${capture_dir}" PHP_VERSION="${php_version}" \
        docker compose --project-directory "${repo_root}" stop collector >/dev/null 2>&1 || true
    case "${capture_dir}" in
        /tmp/otel-php-rust-fork.*) rm -rf -- "${capture_dir}" ;;
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

PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" run --rm \
    -e 'OTEL_EXPORTER_OTLP_ENDPOINT=http://collector:4317' \
    -e 'OTEL_EXPORTER_OTLP_PROTOCOL=grpc' \
    -e 'OTEL_SERVICE_NAME=fork-runtime-parent' \
    -e 'OTEL_BSP_SCHEDULE_DELAY=60000' \
    php php \
    -d extension=/usr/src/myapp/modules/otel.so \
    -d otel.cli.enabled=1 \
    tests/integration/fork_runtime.php

for _ in $(seq 1 30); do
    if [[ -f "${capture_dir}/traces.json" ]] && [[ "$(wc -l < "${capture_dir}/traces.json")" -ge 2 ]]; then
        break
    fi
    sleep 1
done

jq -s -f "${repo_root}/otel/tests/integration/assert_fork_runtime.jq" \
    "${capture_dir}/traces.json"
