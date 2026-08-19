# Shared helpers for the test_otlp_transport_*.sh conformance scripts. Source after
# `set -euo pipefail`; call transport_test_init <capture-prefix> first.

transport_test_init() {
    repo_root="$(cd "$(dirname "${BASH_SOURCE[1]}")/../../.." && pwd)"
    php_version="${PHP_VERSION:-8.2}"
    capture_dir="$(mktemp -d "/tmp/otel-php-rust-$1.XXXXXX")"
    chmod 755 "${capture_dir}"
    started_services=()
    trap transport_test_cleanup EXIT
}

transport_test_cleanup() {
    local status=$?
    if [[ ${#started_services[@]} -gt 0 ]]; then
        if [[ ${status} -ne 0 ]]; then
            compose logs --no-color "${started_services[@]}" 2>/dev/null | tail -n 40 || true
        fi
        compose stop "${started_services[@]}" >/dev/null 2>&1 || true
    fi
    if [[ ${status} -ne 0 && -n "${KEEP_CAPTURE:-}" ]]; then
        echo "capture kept in ${capture_dir}" >&2
        return
    fi
    case "${capture_dir}" in
        /tmp/otel-php-rust-*) rm -rf -- "${capture_dir}" ;;
    esac
}

compose() {
    PHP_VERSION="${php_version}" \
    OTEL_CAPTURE_DIR="${capture_dir}" \
    OTEL_AUTH_CAPTURE_DIR="${capture_dir}" \
    OTEL_TLS_CAPTURE_DIR="${capture_dir}" \
    OTEL_LIMITS_CAPTURE_DIR="${capture_dir}" \
    OTEL_FIXTURE_CAPTURE_DIR="${capture_dir}" \
        docker compose --project-directory "${repo_root}" "$@"
}

# start_services <service>... : (re)creates the services and waits for readiness.
start_services() {
    started_services+=("$@")
    compose up -d --force-recreate "$@"
    local service
    for service in "$@"; do
        case "${service}" in
            collector*) wait_for_log "${service}" 'Everything is ready' ;;
            otlp-fixture) wait_for_log "${service}" 'otlp_transport_fixture:' ;;
        esac
    done
}

wait_for_log() {
    local service="$1" pattern="$2" attempt
    for attempt in $(seq 1 60); do
        if compose logs --no-color "${service}" 2>/dev/null | grep -q "${pattern}"; then
            return 0
        fi
        sleep 1
    done
    echo "service ${service} did not log '${pattern}'" >&2
    return 1
}

# run_probe <case> <protocol> <endpoint> [-e VAR=value ...]
# Runs otlp_transport_probe.php with the extension and prints its JSON result line.
# Diagnostics (otel.log.level=warn) go to ${capture_dir}/probe-<case>.stderr.
run_probe() {
    local case="$1" protocol="$2" endpoint="$3"
    shift 3
    local output
    output="$(compose run --rm -T \
        -v "${capture_dir}:/capture" \
        -e "PROBE_CASE=${case}" \
        -e "PROBE_PLAN=${PROBE_PLAN:-1x0}" \
        -e "OTEL_SERVICE_NAME=transport-${case}" \
        -e "OTEL_EXPORTER_OTLP_PROTOCOL=${protocol}" \
        -e "OTEL_EXPORTER_OTLP_ENDPOINT=${endpoint}" \
        -e 'OTEL_BSP_SCHEDULE_DELAY=60000' \
        -e 'OTEL_EXPORTER_OTLP_TIMEOUT=5000' \
        -e 'OTEL_LOGS_EXPORTER=none' \
        "$@" \
        php php \
        -d "extension=/usr/src/myapp/modules/${OTEL_EXTENSION_SO:-otel.so}" \
        -d otel.cli.enabled=1 \
        -d otel.log.level=warn \
        -d otel.log.file=/dev/stderr \
        tests/integration/otlp_transport_probe.php \
        2>"${capture_dir}/probe-${case}.stderr")"
    printf '%s\n' "${output}" | tail -n 1
}

# assert_probe <case> <json> <jq filter> : the filter must evaluate to true.
assert_probe() {
    local case="$1" json="$2" filter="$3"
    if ! jq -e "${filter}" <<<"${json}" >/dev/null; then
        echo "FAIL [${case}]: ${filter}" >&2
        echo "  result: ${json}" >&2
        if [[ -s "${capture_dir}/probe-${case}.stderr" ]]; then
            echo "  stderr:" >&2
            sed 's/^/    /' "${capture_dir}/probe-${case}.stderr" >&2
        fi
        return 1
    fi
    echo "ok [${case}]: ${filter}"
}

# assert_diagnostic <case> <grep pattern> : the probe's stderr must contain the pattern.
assert_diagnostic() {
    local case="$1" pattern="$2"
    if ! grep -q -- "${pattern}" "${capture_dir}/probe-${case}.stderr"; then
        echo "FAIL [${case}]: expected diagnostic matching '${pattern}'" >&2
        sed 's/^/    /' "${capture_dir}/probe-${case}.stderr" >&2
        return 1
    fi
    echo "ok [${case}]: diagnostic '${pattern}'"
}

assert_no_diagnostic() {
    local case="$1" pattern="$2"
    if grep -q -- "${pattern}" "${capture_dir}/probe-${case}.stderr"; then
        echo "FAIL [${case}]: unexpected diagnostic matching '${pattern}'" >&2
        sed 's/^/    /' "${capture_dir}/probe-${case}.stderr" >&2
        return 1
    fi
    echo "ok [${case}]: no diagnostic '${pattern}'"
}

# generate_test_pki : test-only PKI in ${capture_dir}: ca -> server (SAN collector-tls) and a
# client identity, an unrelated other-ca, and an invalid.pem. Runs openssl in the php image.
generate_test_pki() {
    compose run --rm -T -v "${capture_dir}:/capture" php sh -ec '
        cd /capture
        openssl req -x509 -newkey rsa:2048 -nodes -days 2 -keyout ca.key -out ca.crt \
            -subj /CN=otel-transport-test-ca -addext keyUsage=critical,keyCertSign >/dev/null 2>&1
        openssl req -x509 -newkey rsa:2048 -nodes -days 2 -keyout other-ca.key -out other-ca.crt \
            -subj /CN=otel-transport-other-ca -addext keyUsage=critical,keyCertSign >/dev/null 2>&1
        openssl req -newkey rsa:2048 -nodes -keyout server.key -out server.csr \
            -subj /CN=collector-tls >/dev/null 2>&1
        printf "subjectAltName=DNS:collector-tls\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n" > server.ext
        openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 2 \
            -out server.crt -extfile server.ext >/dev/null 2>&1
        openssl req -newkey rsa:2048 -nodes -keyout client.key -out client.csr \
            -subj /CN=otel-transport-client >/dev/null 2>&1
        printf "extendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\n" > client.ext
        openssl x509 -req -in client.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 2 \
            -out client.crt -extfile client.ext >/dev/null 2>&1
        printf "not a pem file\n" > invalid.pem
        chmod 644 *.key *.crt
    '
}

# fixture_mark : current line count of the fixture log; fixture_records <mark> [jq filter]
# prints the records appended since that mark (one compact JSON object per line).
fixture_mark() {
    if [[ -f "${capture_dir}/fixture.jsonl" ]]; then
        wc -l < "${capture_dir}/fixture.jsonl" | tr -d ' '
    else
        echo 0
    fi
}

fixture_records() {
    local mark="$1" filter="${2:-.}"
    sleep 1
    if [[ -f "${capture_dir}/fixture.jsonl" ]]; then
        tail -n "+$((mark + 1))" "${capture_dir}/fixture.jsonl" | jq -c "${filter}"
    fi
}

# assert_records <case> <records> <jq filter over the array of records>
assert_records() {
    local case="$1" records="$2" filter="$3"
    if ! jq -e -s "${filter}" <<<"${records}" >/dev/null; then
        echo "FAIL [${case}]: fixture records: ${filter}" >&2
        echo "  records: ${records}" >&2
        return 1
    fi
    echo "ok [${case}]: fixture ${filter}"
}

# collector_span_count <service name> : spans the file exporter captured for that service.
collector_span_count() {
    local service="$1"
    if [[ ! -f "${capture_dir}/traces.json" ]]; then
        echo 0
        return
    fi
    jq -s --arg service "${service}" '
        [ .[] | .resourceSpans[]
          | select((.resource.attributes // []) | any(.key == "service.name" and .value.stringValue == $service))
          | .scopeSpans[] | .spans[] ] | length' "${capture_dir}/traces.json"
}

# wait_collector_spans <service> <expected count>
wait_collector_spans() {
    local service="$1" expected="$2" attempt count=0
    for attempt in $(seq 1 30); do
        count="$(collector_span_count "${service}")"
        if [[ "${count}" -ge "${expected}" ]]; then
            break
        fi
        sleep 1
    done
    if [[ "${count}" -ne "${expected}" ]]; then
        echo "FAIL [${service}]: expected ${expected} collector spans, found ${count}" >&2
        return 1
    fi
    echo "ok [${service}]: ${expected} collector span(s)"
}

# assert_no_collector_spans <service> : gives the collector a moment, then requires zero spans.
assert_no_collector_spans() {
    local service="$1" count
    sleep 1
    count="$(collector_span_count "${service}")"
    if [[ "${count}" -ne 0 ]]; then
        echo "FAIL [${service}]: expected no collector spans, found ${count}" >&2
        return 1
    fi
    echo "ok [${service}]: no collector spans"
}
