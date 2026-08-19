.PHONY: build bash integration
.DEFAULT_GOAL: bash

# Verification matrix (docker compose based; needs otel/modules/otel.so from `make build`).
# Benchmarks (run_benchmark_matrix.sh, *_comparison.sh) are run separately on fixed hardware.
INTEGRATION_TESTS = \
	test_reflection_manifest \
	test_batch_nonblocking \
	test_batch_queue_full \
	test_batch_scheduled_export \
	test_batch_export_failure \
	test_batch_export_retry \
	test_otlp_trace_model \
	test_otlp_auth_headers \
	test_otlp_transport_tls \
	test_otlp_transport_wire \
	test_otlp_transport_limits \
	test_request_thread_syscalls \
	test_runtime_thread_bound \
	test_fork_runtime \
	test_long_worker_memory \
	test_messenger_unit_context \
	test_alpine_php82_build \
	test_fpm_lifecycle \
	test_x86_64_musl_build

build-image:
	@echo "Building image..."
	docker compose build
orphans:
	@echo "Cleaning up orphaned containers..."
	docker compose down --remove-orphans
build:
	@echo "Building extension..."
	docker compose run --rm php make build-test
bash:
	docker compose run --rm php bash
clean:
	@echo "Cleaning up..."
	docker compose run --rm php make clean
integration:
	@set -e; for t in $(INTEGRATION_TESTS); do \
		echo "=== $$t"; bash otel/tests/integration/$$t.sh; \
	done; echo "integration: all passed"
