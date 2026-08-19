#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
output="$({
    OTEL_BENCH_REPETITIONS=1 \
        "${repo_root}/otel/tests/integration/run_php_sdk_blackhole_comparison.sh"
})"

jq -e '
    .schema_version == 1
    and .configuration.schedule_delay_ms == 1000
    and .configuration.export_timeout_ms == 3000
    and ([.raw[].engine] | sort == ["opentelemetry-php", "opentelemetry-php", "rust-fork", "rust-fork"])
    and ([.raw[].protocol] | sort == ["grpc", "grpc", "http/protobuf", "http/protobuf"])
    and (all(.raw[];
        .warmup_span_ended == true
        and .warmup_span_recording == true
        and .trigger_span_recording == true
        and .waited_past_schedule == true
        and .trigger_span_end_elapsed_ms >= 0
        and .process_exit_code == 0
    ))
    and ([.medians[].engine] | sort == ["opentelemetry-php", "opentelemetry-php", "rust-fork", "rust-fork"])
    and ([.comparisons[].protocol] | sort == ["grpc", "http/protobuf"])
    and (all(.comparisons[];
        .rust_trigger_span_end_ms >= 0
        and .php_trigger_span_end_ms >= 0
        and .rust_trigger_span_end_ms < 50
        and .php_trigger_span_end_ms >= 500
    ))
' <<<"${output}" >/dev/null

printf '%s\n' "${output}"
