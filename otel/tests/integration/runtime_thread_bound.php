<?php

declare(strict_types=1);

use OpenTelemetry\API\Globals;

$span = Globals::tracerProvider()
    ->getTracer('runtime-thread-test')
    ->spanBuilder('thread-count')
    ->startSpan();
$span->end();

$status = file_get_contents('/proc/self/status') ?: '';
if (preg_match('/^Threads:\s+(\d+)/m', $status, $matches) !== 1) {
    fwrite(STDERR, "Unable to read process thread count\n");
    exit(1);
}

$threads = (int) $matches[1];
if ($threads > 6) {
    fwrite(STDERR, sprintf("Trace runtime created %d threads; limit is 6\n", $threads));
    exit(1);
}

printf("threads: %d\n", $threads);
