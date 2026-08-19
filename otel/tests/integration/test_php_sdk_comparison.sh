#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
output="$({
    OTEL_BENCH_ITERATIONS=2000 \
    OTEL_BENCH_REPETITIONS=1 \
    OTEL_BENCH_INCLUDE_BLACKHOLE=0 \
        "${repo_root}/otel/tests/integration/run_php_sdk_comparison.sh"
})"

jq -e '
    .schema_version == 1
    and .configuration.iterations == 2000
    and .configuration.warmup_iterations == 200
    and .configuration.repetitions == 1
    and .configuration.max_queue_size == 2048
    and .configuration.max_export_batch_size == 512
    and .configuration.schedule_delay_ms == 1000
    and .configuration.export_timeout_ms == 3000
    and (.reference_packages["open-telemetry/sdk"] | test("^[0-9]+\\.[0-9]+\\.[0-9]+$"))
    and (.reference_packages["open-telemetry/exporter-otlp"] | test("^[0-9]+\\.[0-9]+\\.[0-9]+$"))
    and (.reference_packages["open-telemetry/transport-grpc"] | test("^[0-9]+\\.[0-9]+\\.[0-9]+$"))
    and (.raw | length == 6)
    and ([.raw[].engine] | sort == ["opentelemetry-php", "opentelemetry-php", "opentelemetry-php", "rust-fork", "rust-fork", "rust-fork"])
    and ([.raw[].mode] | sort == ["disabled", "disabled", "parentbased_1pct_grpc", "parentbased_1pct_grpc", "parentbased_1pct_http_protobuf", "parentbased_1pct_http_protobuf"])
    and ([.raw[].operation_contract_sha256] | unique | length == 1)
    and (all(.raw[];
        .iterations == 2000
        and .warmup_iterations == 200
        and .ns_per_operation > 0
        and .loop_elapsed_ms > 0
        and .force_flush_elapsed_ms >= 0
        and .provider_setup_elapsed_ms >= 0
        and .peak_rss_kib > 0
        and .threads >= 1
        and (if .mode == "disabled" then .recording_spans == 0 else .recording_spans > 0 end)
    ))
    and ([.medians[].engine] | sort == ["opentelemetry-php", "opentelemetry-php", "opentelemetry-php", "rust-fork", "rust-fork", "rust-fork"])
    and ([.comparisons[].mode] | sort == ["disabled", "parentbased_1pct_grpc", "parentbased_1pct_http_protobuf"])
    and (all(.comparisons[];
        .rust_ns_per_operation > 0
        and .php_ns_per_operation > 0
        and .php_over_rust_loop_ratio > 0
    ))
    and (.runtime.opentelemetry_php.extensions.grpc | test("^[0-9]+\\."))
    and (.runtime.opentelemetry_php.extensions.protobuf | test("^[0-9]+\\."))
    and .runtime.rust_fork.php == .runtime.opentelemetry_php.php
    and (all(.raw[] | select(.engine == "opentelemetry-php" and .mode != "disabled");
        .force_flush_result == true
    ))
' <<<"${output}" >/dev/null

printf '%s\n' "${output}"
