#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
image="otel-php-rust:php82-alpine-local"

docker build \
    --file "${repo_root}/Dockerfile.alpine" \
    --build-arg PHP_VERSION=8.2 \
    --build-arg ALPINE_VERSION=3.18 \
    --build-arg RUST_VERSION=1.97.1 \
    --tag "${image}" \
    "${repo_root}"

docker run --rm "${image}" \
    -n \
    -d extension=/usr/local/lib/php/extensions/otel.so \
    -r 'var_dump(PHP_VERSION, PHP_SAPI, extension_loaded("otel"));'
