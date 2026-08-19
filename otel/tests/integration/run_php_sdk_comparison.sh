#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
php_version="${PHP_VERSION:-8.2}"
iterations="${OTEL_BENCH_ITERATIONS:-100000}"
warmup_iterations=$((iterations / 10))
if ((warmup_iterations < 100)); then
    warmup_iterations=100
fi
repetitions="${OTEL_BENCH_REPETITIONS:-5}"
results_file="$(mktemp /tmp/otel-php-sdk-comparison.XXXXXX)"
cases=(
    'rust-fork|disabled'
    'opentelemetry-php|disabled'
    'rust-fork|parentbased_1pct_grpc'
    'opentelemetry-php|parentbased_1pct_grpc'
    'rust-fork|parentbased_1pct_http_protobuf'
    'opentelemetry-php|parentbased_1pct_http_protobuf'
)

cleanup() {
    PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" \
        stop collector-benchmark >/dev/null 2>&1 || true
    case "${results_file}" in
        /tmp/otel-php-sdk-comparison.*) rm -f -- "${results_file}" ;;
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
    up -d --force-recreate collector-benchmark >/dev/null

reference_packages="$(jq '
    [.packages[]
        | select(.name == "open-telemetry/api"
            or .name == "open-telemetry/sdk"
            or .name == "open-telemetry/exporter-otlp"
            or .name == "open-telemetry/transport-grpc")
        | {key: .name, value: .version}]
    | from_entries
' "${repo_root}/otel/tests/integration/opentelemetry-php/composer.lock")"

rust_runtime="$(PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" run --rm \
    php php -d extension=/usr/src/myapp/modules/otel.so -r \
    'echo json_encode(["php" => PHP_VERSION, "extension" => phpversion("otel")], JSON_THROW_ON_ERROR);')"
php_runtime="$(PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" run --rm \
    php-official php -r \
    'echo json_encode(["php" => PHP_VERSION, "extensions" => ["grpc" => phpversion("grpc"), "protobuf" => phpversion("protobuf")]], JSON_THROW_ON_ERROR);')"

run_case() {
    local engine="$1"
    local mode="$2"
    local protocol=''
    local endpoint=''

    case "${mode}" in
        parentbased_1pct_grpc)
            protocol='grpc'
            endpoint='http://collector-benchmark:4317'
            ;;
        parentbased_1pct_http_protobuf)
            protocol='http/protobuf'
            endpoint='http://collector-benchmark:4318'
            ;;
    esac

    local common=(
        docker compose --project-directory "${repo_root}" run --rm
        -e "OTEL_BENCH_ENGINE=${engine}"
        -e "OTEL_BENCH_MODE=${mode}"
        -e "OTEL_BENCH_ITERATIONS=${iterations}"
        -e OTEL_BSP_MAX_QUEUE_SIZE=2048
        -e OTEL_BSP_MAX_EXPORT_BATCH_SIZE=512
        -e OTEL_BSP_SCHEDULE_DELAY=1000
        -e OTEL_BSP_EXPORT_TIMEOUT=3000
        -e OTEL_LOGS_EXPORTER=none
    )

    if [[ "${mode}" == 'disabled' ]]; then
        if [[ "${engine}" == 'rust-fork' ]]; then
            PHP_VERSION="${php_version}" "${common[@]}" \
                -e OTEL_SDK_DISABLED=true \
                php php -d extension=/usr/src/myapp/modules/otel.so -d otel.cli.enabled=1 \
                tests/integration/benchmark_manual_spans.php
        else
            PHP_VERSION="${php_version}" "${common[@]}" \
                -e OTEL_BENCH_PROVIDER_BOOTSTRAP=/usr/src/myapp/tests/integration/opentelemetry_php_bootstrap.php \
                php-official php tests/integration/benchmark_manual_spans.php
        fi
        return
    fi

    if [[ "${engine}" == 'rust-fork' ]]; then
        PHP_VERSION="${php_version}" "${common[@]}" \
            -e "OTEL_EXPORTER_OTLP_ENDPOINT=${endpoint}" \
            -e "OTEL_EXPORTER_OTLP_PROTOCOL=${protocol}" \
            -e OTEL_TRACES_SAMPLER=parentbased_traceidratio \
            -e OTEL_TRACES_SAMPLER_ARG=0.01 \
            php php -d extension=/usr/src/myapp/modules/otel.so -d otel.cli.enabled=1 \
            tests/integration/benchmark_manual_spans.php
    else
        PHP_VERSION="${php_version}" "${common[@]}" \
            -e "OTEL_EXPORTER_OTLP_ENDPOINT=${endpoint}" \
            -e "OTEL_EXPORTER_OTLP_PROTOCOL=${protocol}" \
            -e OTEL_TRACES_SAMPLER=parentbased_traceidratio \
            -e OTEL_TRACES_SAMPLER_ARG=0.01 \
            -e OTEL_BENCH_EXPORT_TIMEOUT_SECONDS=3 \
            -e OTEL_BENCH_PROVIDER_BOOTSTRAP=/usr/src/myapp/tests/integration/opentelemetry_php_bootstrap.php \
            php-official php tests/integration/benchmark_manual_spans.php
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
        IFS='|' read -r engine mode <<<"${benchmark_case}"
        result="$(run_case "${engine}" "${mode}" | grep '^{' | tail -n 1)"
        jq -c --argjson repetition "${repetition}" '. + {repetition: $repetition}' \
            <<<"${result}" >>"${results_file}"
    done
done

jq -s \
    --argjson reference_packages "${reference_packages}" \
    --argjson rust_runtime "${rust_runtime}" \
    --argjson php_runtime "${php_runtime}" \
    --argjson iterations "${iterations}" \
    --argjson warmup_iterations "${warmup_iterations}" \
    --argjson repetitions "${repetitions}" '
    def median:
        sort
        | length as $length
        | if ($length % 2) == 1 then .[$length / 2 | floor]
          else (.[($length / 2) - 1] + .[$length / 2]) / 2
          end;
    def medians:
        group_by([.engine, .mode])
        | map({
            engine: .[0].engine,
            mode: .[0].mode,
            ns_per_operation: (map(.ns_per_operation) | median),
            loop_elapsed_ms: (map(.loop_elapsed_ms) | median),
            force_flush_elapsed_ms: (map(.force_flush_elapsed_ms) | median),
            provider_setup_elapsed_ms: (map(.provider_setup_elapsed_ms) | median),
            peak_rss_kib: (map(.peak_rss_kib) | median),
            threads: (map(.threads) | median)
        });
    . as $raw
    | ($raw | medians) as $medians
    | {
        schema_version: 1,
        configuration: {
            php_version: $rust_runtime.php,
            iterations: $iterations,
            warmup_iterations: $warmup_iterations,
            repetitions: $repetitions,
            sampler: "parentbased_traceidratio",
            sampler_argument: 0.01,
            max_queue_size: 2048,
            max_export_batch_size: 512,
            schedule_delay_ms: 1000,
            export_timeout_ms: 3000
        },
        reference_packages: $reference_packages,
        runtime: {
            rust_fork: $rust_runtime,
            opentelemetry_php: $php_runtime
        },
        raw: $raw,
        medians: $medians,
        comparisons: ([
            "disabled",
            "parentbased_1pct_grpc",
            "parentbased_1pct_http_protobuf"
        ] | map(. as $mode
            | ($medians[] | select(.engine == "rust-fork" and .mode == $mode)) as $rust
            | ($medians[] | select(.engine == "opentelemetry-php" and .mode == $mode)) as $php
            | {
                mode: $mode,
                rust_ns_per_operation: $rust.ns_per_operation,
                php_ns_per_operation: $php.ns_per_operation,
                php_over_rust_loop_ratio: ($php.ns_per_operation / $rust.ns_per_operation),
                rust_loop_savings_percent: ((1 - ($rust.ns_per_operation / $php.ns_per_operation)) * 100),
                rust_force_flush_ms: $rust.force_flush_elapsed_ms,
                php_force_flush_ms: $php.force_flush_elapsed_ms,
                rust_peak_rss_kib: $rust.peak_rss_kib,
                php_peak_rss_kib: $php.peak_rss_kib
            }
        ))
    }
' "${results_file}"
