<?php

declare(strict_types=1);

use OpenTelemetry\API\Globals;

$tracer = Globals::tracerProvider()->getTracer('batch-nonblocking-test');
$startedAt = hrtime(true);

for ($i = 0; $i < 1; ++$i) {
    $tracer
        ->spanBuilder('nonblocking-' . $i)
        ->startSpan()
        ->end();
}

$elapsedMilliseconds = (hrtime(true) - $startedAt) / 1_000_000;
if ($elapsedMilliseconds >= 200) {
    fwrite(STDERR, sprintf("Span::end() path blocked for %.3f ms\n", $elapsedMilliseconds));
    exit(1);
}

printf("span end: %.3f ms\n", $elapsedMilliseconds);
