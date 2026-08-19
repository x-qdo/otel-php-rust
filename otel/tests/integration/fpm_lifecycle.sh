#!/bin/sh
# Runs inside the Alpine FPM image: drives php-fpm through reload (USR2), graceful (QUIT) and
# forced (TERM) termination while requests are served, and reports a JSON summary. The
# extension must keep exporting across worker recycling and never crash a worker.
set -eu

REQUESTS="${FPM_REQUESTS:-60}"
OTEL_LOG_LEVEL="${FPM_OTEL_LOG_LEVEL:-warn}"
apk add --no-cache fcgi procps >/dev/null 2>&1

# The FPM pool inherits the OTEL_* environment of this script (clear_env = no).
start_fpm() {
    ready_before="$(grep -c "ready to handle connections" /var/log/php-fpm.log 2>/dev/null || true)"
    php-fpm -y /srv/fpm_pool.conf -F \
        -d extension=/usr/local/lib/php/extensions/otel.so \
        -d "otel.log.level=${OTEL_LOG_LEVEL}" -d otel.log.file=/proc/self/fd/2 \
        >>/var/log/php-fpm.log 2>&1 &
    FPM_PID=$!
    for _ in $(seq 1 50); do
        ready_now="$(grep -c "ready to handle connections" /var/log/php-fpm.log 2>/dev/null || true)"
        if [ "$ready_now" -gt "$ready_before" ]; then
            return 0
        fi
        sleep 0.1
    done
    echo "php-fpm did not become ready" >&2
    cat /var/log/php-fpm.log >&2
    exit 1
}

request() {
    SCRIPT_FILENAME=/srv/fpm_request.php REQUEST_METHOD=GET \
        timeout 10 cgi-fcgi -bind -connect 127.0.0.1:9000 2>/dev/null | tail -n 1
}

# fire <count> : serves requests and records the distinct worker pids that answered.
fire() {
    count="$1"; ok=0
    for _ in $(seq 1 "$count"); do
        line="$(request || true)"
        case "$line" in
            '{"pid":'*) ok=$((ok + 1)); echo "$line" >> /tmp/responses.jsonl; echo "$line" | sed 's/.*"pid":\([0-9]*\).*/\1/' >> /tmp/pids.txt ;;
            *) echo "bad response: $line" >> /tmp/errors.txt ;;
        esac
    done
    echo "$ok"
}

now_ms() {
    awk '{ printf "%d", $1 * 1000 }' /proc/uptime
}

# wait_exit <pid> <limit deciseconds> : the child is a zombie until reaped, so poll its
# state instead of kill -0; sets WAIT_RESULT to exit=<code> or timeout. Must run in the
# main shell (not a command substitution) so `wait` can reap the child.
wait_exit() {
    pid="$1"; limit_ds="$2"; waited=0
    while [ -d "/proc/$pid" ] && ! grep -q '^State:.*Z' "/proc/$pid/status" 2>/dev/null; do
        if [ "$waited" -ge "$limit_ds" ]; then
            WAIT_RESULT="timeout"; return 0
        fi
        sleep 0.1; waited=$((waited + 1))
    done
    if wait "$pid"; then WAIT_RESULT="exit=0"; else WAIT_RESULT="exit=$?"; fi
}

: > /tmp/pids.txt; : > /tmp/errors.txt; : > /tmp/responses.jsonl; : > /var/log/php-fpm.log

start_fpm
phase1_ok="$(fire "$REQUESTS")"
# PHP-FPM workers may be reaped with _exit() and do not reliably run MSHUTDOWN.
# Let the bounded exporter worker drain completed requests before recycling the
# process; the forced phase below intentionally omits this drain window.
sleep 0.5

# Reload: the master re-forks workers; in-flight requests finish, new requests are served.
ready_before="$(grep -c 'ready to handle connections' /var/log/php-fpm.log)"
kill -USR2 "$FPM_PID"
sleep 0.5
for _ in $(seq 1 50); do
    if [ "$(grep -c 'ready to handle connections' /var/log/php-fpm.log)" -gt "$ready_before" ]; then break; fi
    sleep 0.1
done
phase2_ok="$(fire "$REQUESTS")"
sleep 0.5

# Graceful termination: active workers finish and the master exits within the budget.
graceful_started=$(now_ms)
kill -QUIT "$FPM_PID"
wait_exit "$FPM_PID" 100; graceful_result="$WAIT_RESULT"
graceful_ms=$(( $(now_ms) - graceful_started ))

# Forced termination after more traffic.
start_fpm
phase3_ok="$(fire "$REQUESTS")"
forced_started=$(now_ms)
kill -TERM "$FPM_PID"
wait_exit "$FPM_PID" 50; forced_result="$WAIT_RESULT"
forced_ms=$(( $(now_ms) - forced_started ))

crashes="$(grep -cE 'exited on signal|SIGSEGV|SIGABRT|core dumped' /var/log/php-fpm.log || true)"
leftover="$(pgrep -c php-fpm || true)"
distinct_pids="$(sort -u /tmp/pids.txt | wc -l | tr -d ' ')"

printf '{"requests_per_phase":%s,"phase1_ok":%s,"phase2_ok":%s,"phase3_ok":%s,"distinct_worker_pids":%s,"graceful":"%s","graceful_ms":%s,"forced":"%s","forced_ms":%s,"crashes":%s,"leftover_processes":%s,"bad_responses":%s}\n' \
    "$REQUESTS" "$phase1_ok" "$phase2_ok" "$phase3_ok" "$distinct_pids" "$graceful_result" "$graceful_ms" "$forced_result" "$forced_ms" "$crashes" "$leftover" "$(wc -l < /tmp/errors.txt | tr -d ' ')"
cat /var/log/php-fpm.log >&2
cat /tmp/responses.jsonl >&2
