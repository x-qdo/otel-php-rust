#!/usr/bin/env bash
# Two-process reflection manifest check: dump the public surface of the
# Composer-locked open-telemetry/api + open-telemetry/context packages in one
# PHP process (no extension), dump what the otel extension declares in a second
# process (no Composer), then compare both under tests/reflection/policy.json.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
php_version="${PHP_VERSION:-8.2}"
out_dir="target/reflection"

compose() {
    PHP_VERSION="${php_version}" docker compose --project-directory "${repo_root}" run --rm -T "$@"
}

compose php sh -c "mkdir -p ${out_dir} && cd tests/reflection && composer install --no-interaction --no-progress --no-dev --quiet"

compose php php -n \
    tests/reflection/dump_manifest.php official tests/reflection/vendor "${out_dir}/official.json"

compose php php -n -d extension=/usr/src/myapp/modules/otel.so \
    tests/reflection/dump_manifest.php extension "${out_dir}/official.json" "${out_dir}/extension.json"

compose php php -n \
    tests/reflection/compare_manifest.php "${out_dir}/official.json" "${out_dir}/extension.json" tests/reflection/policy.json
