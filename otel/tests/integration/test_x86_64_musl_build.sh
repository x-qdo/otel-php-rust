#!/usr/bin/env bash
# Cross-architecture release build: builds the x86_64-musl artifact from Dockerfile.alpine
# (via buildx emulation on non-x86 hosts), loads it in the matching x86_64 PHP 8.2 FPM
# Alpine image, records the ABI metadata and proves a span records end to end.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
out_dir="$(mktemp -d /tmp/otel-php-rust-x86_64.XXXXXX)"
trap 'rm -rf -- "${out_dir}"' EXIT
runtime_image="php:8.2-fpm-alpine3.18"

docker buildx build \
    --platform linux/amd64 \
    --target artifact \
    --output "type=local,dest=${out_dir}" \
    --build-arg PHP_VERSION=8.2 \
    --build-arg ALPINE_VERSION=3.18 \
    --build-arg RUST_VERSION=1.97.1 \
    --file "${repo_root}/Dockerfile.alpine" \
    "${repo_root}" >/dev/null

docker pull --platform linux/amd64 "${runtime_image}" >/dev/null
runtime_digest="$(docker image inspect --format '{{index .RepoDigests 0}}' "${runtime_image}")"

result="$(docker run --rm --platform linux/amd64 \
    -e OTEL_ARTIFACT_LIBC=musl \
    -e OTEL_ARTIFACT_RUNTIME_IMAGE="${runtime_image}" \
    -e OTEL_ARTIFACT_RUNTIME_IMAGE_DIGEST="${runtime_digest}" \
    -v "${out_dir}/otel.so:/otel.so:ro" \
    -v "${repo_root}/otel/tests/integration/abi_metadata.php:/abi_metadata.php:ro" \
    "${runtime_image}" sh -ec '
        apk add --no-cache libgcc >/dev/null
        export OTEL_ARTIFACT_LIBC_VERSION="$(ldd --version 2>&1 | head -n 1 || true)"
        php -n -d extension=/otel.so /abi_metadata.php
        php -n -d extension=/otel.so -d otel.cli.enabled=1 -r '"'"'
            $provider = OpenTelemetry\API\Globals::tracerProvider();
            $span = $provider->getTracer("x86_64-musl")->spanBuilder("load-test")->startSpan();
            $recording = $span->isRecording();
            $span->end();
            echo json_encode(["recording" => $recording, "metrics" => $provider->getRuntimeMetrics()]), "\n";
        '"'"'
    ' 2>/dev/null)"
echo "${result}"
abi="$(echo "${result}" | sed -n '/^{$/,/^}$/p')"
probe="$(echo "${result}" | tail -n 1)"
jq -e '.arch == "x86_64" and .libc == "musl" and .linkage == "dynamic" and (.runtime_image_digest | contains("@sha256:")) and .zend_module_abi == "no-debug-non-zts-20220829" and .php_version_id >= 80200 and .php_version_id < 80300' <<<"${abi}" >/dev/null
jq -e '.recording == true and .metrics.sampled_ended == 1' <<<"${probe}" >/dev/null
echo "test_x86_64_musl_build: x86_64-musl artifact loads in php:8.2-fpm-alpine3.18 and records spans"
