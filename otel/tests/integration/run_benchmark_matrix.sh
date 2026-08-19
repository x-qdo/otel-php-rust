#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
php_version="${PHP_VERSION:-8.2}"
iterations="${OTEL_BENCH_ITERATIONS:-100000}"
repetitions="${OTEL_BENCH_REPETITIONS:-5}"
results_file="$(mktemp /tmp/otel-php-rust-benchmark.XXXXXX)"
modes=(
    baseline
    loaded_disabled
    parentbased_1pct_grpc
    parentbased_1pct_http_protobuf
    always_on_grpc
    always_on_http_protobuf
)

cleanup() {
    PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" \
        stop collector-benchmark >/dev/null 2>&1 || true
    case "${results_file}" in
        /tmp/otel-php-rust-benchmark.*) rm -f -- "${results_file}" ;;
    esac
}
trap cleanup EXIT

PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" \
    run --rm php make build >/dev/null

PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" \
    up -d --force-recreate collector-benchmark

run_mode() {
    local mode="$1"
    local common=(
        docker compose --project-directory "${repo_root}" run --rm
        -e "OTEL_BENCH_MODE=${mode}"
        -e "OTEL_BENCH_ITERATIONS=${iterations}"
    )

    case "${mode}" in
        baseline)
            PHP_VERSION="${php_version}" "${common[@]}" php \
                php tests/integration/benchmark_manual_spans.php
            ;;
        loaded_disabled)
            PHP_VERSION="${php_version}" "${common[@]}" \
                -e OTEL_SDK_DISABLED=true \
                -e OTEL_LOGS_EXPORTER=none \
                php php -d extension=/usr/src/myapp/modules/otel.so -d otel.cli.enabled=1 \
                tests/integration/benchmark_manual_spans.php
            ;;
        parentbased_1pct_grpc)
            PHP_VERSION="${php_version}" "${common[@]}" \
                -e OTEL_EXPORTER_OTLP_ENDPOINT=http://collector-benchmark:4317 \
                -e OTEL_EXPORTER_OTLP_PROTOCOL=grpc \
                -e OTEL_LOGS_EXPORTER=none \
                -e OTEL_TRACES_SAMPLER=parentbased_traceidratio \
                -e OTEL_TRACES_SAMPLER_ARG=0.01 \
                php php -d extension=/usr/src/myapp/modules/otel.so -d otel.cli.enabled=1 \
                tests/integration/benchmark_manual_spans.php
            ;;
        parentbased_1pct_http_protobuf)
            PHP_VERSION="${php_version}" "${common[@]}" \
                -e OTEL_EXPORTER_OTLP_ENDPOINT=http://collector-benchmark:4318 \
                -e OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf \
                -e OTEL_LOGS_EXPORTER=none \
                -e OTEL_TRACES_SAMPLER=parentbased_traceidratio \
                -e OTEL_TRACES_SAMPLER_ARG=0.01 \
                php php -d extension=/usr/src/myapp/modules/otel.so -d otel.cli.enabled=1 \
                tests/integration/benchmark_manual_spans.php
            ;;
        always_on_grpc)
            PHP_VERSION="${php_version}" "${common[@]}" \
                -e OTEL_EXPORTER_OTLP_ENDPOINT=http://collector-benchmark:4317 \
                -e OTEL_EXPORTER_OTLP_PROTOCOL=grpc \
                -e OTEL_LOGS_EXPORTER=none \
                -e OTEL_TRACES_SAMPLER=always_on \
                php php -d extension=/usr/src/myapp/modules/otel.so -d otel.cli.enabled=1 \
                tests/integration/benchmark_manual_spans.php
            ;;
        always_on_http_protobuf)
            PHP_VERSION="${php_version}" "${common[@]}" \
                -e OTEL_EXPORTER_OTLP_ENDPOINT=http://collector-benchmark:4318 \
                -e OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf \
                -e OTEL_LOGS_EXPORTER=none \
                -e OTEL_TRACES_SAMPLER=always_on \
                php php -d extension=/usr/src/myapp/modules/otel.so -d otel.cli.enabled=1 \
                tests/integration/benchmark_manual_spans.php
            ;;
    esac
}

for ((repetition = 1; repetition <= repetitions; ++repetition)); do
    order=("${modes[@]}")
    for ((index = ${#order[@]} - 1; index > 0; --index)); do
        swap_index=$((RANDOM % (index + 1)))
        temporary="${order[index]}"
        order[index]="${order[swap_index]}"
        order[swap_index]="${temporary}"
    done

    for mode in "${order[@]}"; do
        result="$(run_mode "${mode}" | tail -n 1)"
        printf '%s\n' "${result}" >> "${results_file}"
    done
done

jq -s '
    def median:
        sort
        | length as $length
        | if ($length % 2) == 1 then .[$length / 2 | floor]
          else (.[($length / 2) - 1] + .[$length / 2]) / 2
          end;
    . as $raw
    | {
        raw: $raw,
        medians: (
            group_by(.mode)
            | map({
                mode: .[0].mode,
                elapsed_ms: (map(.elapsed_ms) | median),
                ns_per_operation: (map(.ns_per_operation) | median),
                peak_rss_kib: (map(.peak_rss_kib) | median),
                threads: (map(.threads) | median),
                sampled_ended: (map(.runtime_metrics.sampled_ended // 0) | median),
                exported: (map(.runtime_metrics.exported // 0) | median),
                dropped_queue_full: (map(.runtime_metrics.dropped_queue_full // 0) | median),
                export_failures: (map(.runtime_metrics.export_failures // 0) | median)
            })
        )
      }
' "${results_file}"
