#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
php_version="${PHP_VERSION:-8.2}"
repetitions="${OTEL_BENCH_REPETITIONS:-5}"
results_file="$(mktemp /tmp/otel-php-sdk-blackhole.XXXXXX)"
cases=(
    'rust-fork|grpc|4317'
    'opentelemetry-php|grpc|4317'
    'rust-fork|http/protobuf|4318'
    'opentelemetry-php|http/protobuf|4318'
)

cleanup() {
    PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" \
        stop blackhole >/dev/null 2>&1 || true
    case "${results_file}" in
        /tmp/otel-php-sdk-blackhole.*) rm -f -- "${results_file}" ;;
    esac
}
trap cleanup EXIT

if [[ "${OTEL_BENCH_SKIP_BUILD:-0}" != '1' ]]; then
    PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" \
        run --rm php make build >/dev/null
    DOCKER_BUILDKIT=0 PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" \
        build php-official >/dev/null
fi

PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" \
    up -d --force-recreate blackhole >/dev/null

run_case() {
    local engine="$1"
    local protocol="$2"
    local port="$3"
    local common=(
        docker compose --project-directory "${repo_root}" run --rm
        -e "OTEL_BENCH_ENGINE=${engine}"
        -e OTEL_BENCH_MODE=scheduled_blackhole
        -e "OTEL_EXPORTER_OTLP_ENDPOINT=http://blackhole:${port}"
        -e "OTEL_EXPORTER_OTLP_PROTOCOL=${protocol}"
        -e OTEL_TRACES_SAMPLER=always_on
        -e OTEL_BSP_MAX_QUEUE_SIZE=2048
        -e OTEL_BSP_MAX_EXPORT_BATCH_SIZE=512
        -e OTEL_BSP_SCHEDULE_DELAY=1000
        -e OTEL_BSP_EXPORT_TIMEOUT=3000
        -e OTEL_LOGS_EXPORTER=none
    )

    if [[ "${engine}" == 'rust-fork' ]]; then
        PHP_VERSION="${php_version}" "${common[@]}" \
            -e OTEL_EXPORTER_OTLP_TIMEOUT=750 \
            php php -d extension=/usr/src/myapp/modules/otel.so -d otel.cli.enabled=1 \
            tests/integration/benchmark_scheduled_blackhole.php
    else
        PHP_VERSION="${php_version}" "${common[@]}" \
            -e OTEL_BENCH_EXPORT_TIMEOUT_SECONDS=0.75 \
            -e OTEL_BENCH_PROVIDER_BOOTSTRAP=/usr/src/myapp/tests/integration/opentelemetry_php_bootstrap.php \
            php-official php tests/integration/benchmark_scheduled_blackhole.php
    fi
}

for ((repetition = 1; repetition <= repetitions; ++repetition)); do
    order=("${cases[@]}")
    for ((index = ${#order[@]} - 1; index > 0; --index)); do
        swap_index=$((RANDOM % (index + 1)))
        temporary="${order[index]}"
        order[index]="${order[swap_index]}"
        order[swap_index]="${temporary}"
    done

    for benchmark_case in "${order[@]}"; do
        IFS='|' read -r engine protocol port <<<"${benchmark_case}"
        set +e
        output="$(run_case "${engine}" "${protocol}" "${port}")"
        process_exit_code=$?
        set -e
        result="$(grep '^{' <<<"${output}" | tail -n 1)"
        jq -c \
            --argjson repetition "${repetition}" \
            --argjson process_exit_code "${process_exit_code}" \
            '. + {repetition: $repetition, process_exit_code: $process_exit_code}' \
            <<<"${result}" >>"${results_file}"
    done
done

jq -s --argjson repetitions "${repetitions}" '
    def median:
        sort
        | length as $length
        | if ($length % 2) == 1 then .[$length / 2 | floor]
          else (.[($length / 2) - 1] + .[$length / 2]) / 2
          end;
    . as $raw
    | ($raw
        | group_by([.engine, .protocol])
        | map({
            engine: .[0].engine,
            protocol: .[0].protocol,
            trigger_span_end_elapsed_ms: (map(.trigger_span_end_elapsed_ms) | median),
            process_exit_code: (map(.process_exit_code) | max)
        })) as $medians
    | {
        schema_version: 1,
        configuration: {
            repetitions: $repetitions,
            schedule_delay_ms: 1000,
            export_timeout_ms: 3000,
            transport_timeout_ms: 750
        },
        raw: $raw,
        medians: $medians,
        comparisons: (["grpc", "http/protobuf"] | map(. as $protocol
            | ($medians[] | select(.engine == "rust-fork" and .protocol == $protocol)) as $rust
            | ($medians[] | select(.engine == "opentelemetry-php" and .protocol == $protocol)) as $php
            | {
                protocol: $protocol,
                rust_trigger_span_end_ms: $rust.trigger_span_end_elapsed_ms,
                php_trigger_span_end_ms: $php.trigger_span_end_elapsed_ms,
                php_minus_rust_ms: ($php.trigger_span_end_elapsed_ms - $rust.trigger_span_end_elapsed_ms)
            }
        ))
    }
' "${results_file}"
