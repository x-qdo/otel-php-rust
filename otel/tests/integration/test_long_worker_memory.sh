#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
php_version="${PHP_VERSION:-8.2}"

PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" run --rm \
    -e OTEL_SDK_DISABLED=true \
    -e OTEL_LOGS_EXPORTER=none \
    php php \
    -d extension=/usr/src/myapp/modules/otel.so \
    -d otel.cli.enabled=1 \
    tests/integration/long_worker_memory.php
