--TEST--
Contained panics are logged with message and location, then suppressed after the per-process limit
--EXTENSIONS--
otel
--SKIPIF--
<?php if (!function_exists('otel_test_panic')) die('skip requires a --features test build'); ?>
--INI--
otel.cli.enabled=1
otel.log.level="error"
otel.log.file="/dev/stdout"
--ENV--
OTEL_TRACES_EXPORTER=none
--FILE--
<?php
$caught = 0;
for ($i = 0; $i < 13; $i++) {
    try {
        otel_test_panic('function');
    } catch (\Error $e) {
        $caught++;
    }
}
var_dump($caught);
echo "done\n";
?>
--EXPECTF--
[%s] [ERROR] [pid=%d] [ThreadId(%d)] otel::panic: event src/panic.rs:%d message=internal panic contained: test panic in function at src/panic.rs:%d:%d
[%s] [ERROR] [pid=%d] [ThreadId(%d)] otel::panic: event src/panic.rs:%d message=internal panic contained: test panic in function at src/panic.rs:%d:%d
[%s] [ERROR] [pid=%d] [ThreadId(%d)] otel::panic: event src/panic.rs:%d message=internal panic contained: test panic in function at src/panic.rs:%d:%d
[%s] [ERROR] [pid=%d] [ThreadId(%d)] otel::panic: event src/panic.rs:%d message=internal panic contained: test panic in function at src/panic.rs:%d:%d
[%s] [ERROR] [pid=%d] [ThreadId(%d)] otel::panic: event src/panic.rs:%d message=internal panic contained: test panic in function at src/panic.rs:%d:%d
[%s] [ERROR] [pid=%d] [ThreadId(%d)] otel::panic: event src/panic.rs:%d message=internal panic contained: test panic in function at src/panic.rs:%d:%d
[%s] [ERROR] [pid=%d] [ThreadId(%d)] otel::panic: event src/panic.rs:%d message=internal panic contained: test panic in function at src/panic.rs:%d:%d
[%s] [ERROR] [pid=%d] [ThreadId(%d)] otel::panic: event src/panic.rs:%d message=internal panic contained: test panic in function at src/panic.rs:%d:%d
[%s] [ERROR] [pid=%d] [ThreadId(%d)] otel::panic: event src/panic.rs:%d message=internal panic contained: test panic in function at src/panic.rs:%d:%d
[%s] [ERROR] [pid=%d] [ThreadId(%d)] otel::panic: event src/panic.rs:%d message=internal panic contained: test panic in function at src/panic.rs:%d:%d
[%s] [ERROR] [pid=%d] [ThreadId(%d)] otel::panic: event src/panic.rs:%d message=internal panic contained; further panic diagnostics are suppressed for this process (limit 10)
int(13)
done
