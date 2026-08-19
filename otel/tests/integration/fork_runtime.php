<?php

declare(strict_types=1);

use OpenTelemetry\API\Globals;

$parentProvider = Globals::tracerProvider();
$parentProvider
    ->getTracer('fork-runtime-test')
    ->spanBuilder('parent-process-span')
    ->startSpan()
    ->end();
$parentProvider->forceFlush();

$childPid = pcntl_fork();
if ($childPid === -1) {
    throw new RuntimeException('Unable to fork test process');
}

if ($childPid === 0) {
    putenv('OTEL_SERVICE_NAME=fork-runtime-child');
    $childProvider = Globals::tracerProvider();
    $childSpan = $childProvider
        ->getTracer('fork-runtime-test')
        ->spanBuilder('child-process-span')
        ->startSpan();
    if (!$childSpan->isRecording()) {
        fwrite(STDERR, "Child process received a non-recording provider\n");
        exit(2);
    }
    $childSpan->end();
    $childProvider->forceFlush();
    exit(0);
}

pcntl_waitpid($childPid, $status);
if (!pcntl_wifexited($status) || pcntl_wexitstatus($status) !== 0) {
    throw new RuntimeException(sprintf('Child process failed with status %d', $status));
}

echo "fork export complete\n";
