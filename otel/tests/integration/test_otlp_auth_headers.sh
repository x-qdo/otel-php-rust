#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
capture_dir="$(mktemp -d /tmp/otel-php-rust-auth-capture.XXXXXX)"
php_version="${PHP_VERSION:-8.2}"

cleanup() {
    OTEL_AUTH_CAPTURE_DIR="${capture_dir}" PHP_VERSION="${php_version}" \
        docker compose --project-directory "${repo_root}" stop collector-auth >/dev/null 2>&1 || true
    case "${capture_dir}" in
        /tmp/otel-php-rust-auth-capture.*) rm -rf -- "${capture_dir}" ;;
    esac
}
trap cleanup EXIT

OTEL_AUTH_CAPTURE_DIR="${capture_dir}" PHP_VERSION="${php_version}" \
    docker compose --project-directory "${repo_root}" up -d --force-recreate collector-auth

for _ in $(seq 1 30); do
    if OTEL_AUTH_CAPTURE_DIR="${capture_dir}" PHP_VERSION="${php_version}" \
        docker compose --project-directory "${repo_root}" logs --no-color collector-auth \
        | grep -q 'Everything is ready'; then
        break
    fi
    sleep 1
done

run_export() {
    local protocol="$1"
    local port="$2"
    local service="$3"
    local source="$4"
    shift 4

    PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" run --rm \
        -e "OTEL_EXPORTER_OTLP_ENDPOINT=http://collector-auth:${port}" \
        -e "OTEL_EXPORTER_OTLP_PROTOCOL=${protocol}" \
        -e "OTEL_SERVICE_NAME=${service}" \
        -e "AUTH_HEADER_SOURCE=${source}" \
        -e 'OTEL_LOGS_EXPORTER=none' \
        "$@" \
        php php \
        -d extension=/usr/src/myapp/modules/otel.so \
        -d otel.cli.enabled=1 \
        tests/integration/otlp_auth_headers.php
}

run_export grpc 4317 auth-grpc-global-header global \
    -e 'OTEL_EXPORTER_OTLP_HEADERS=Authorization=Bearer%20trace-secret'
run_export http/protobuf 4318 auth-http-traces-header traces-specific \
    -e 'OTEL_EXPORTER_OTLP_HEADERS=Authorization=Bearer%20wrong-secret' \
    -e 'OTEL_EXPORTER_OTLP_TRACES_HEADERS=Authorization=Bearer%20trace-secret'

for _ in $(seq 1 30); do
    if [[ -f "${capture_dir}/traces.json" ]] && [[ "$(wc -l < "${capture_dir}/traces.json")" -ge 2 ]]; then
        break
    fi
    sleep 1
done

jq -s -f "${repo_root}/otel/tests/integration/assert_otlp_auth_headers.jq" \
    "${capture_dir}/traces.json"
