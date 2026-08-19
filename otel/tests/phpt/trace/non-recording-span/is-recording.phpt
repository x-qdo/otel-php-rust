--TEST--
Non-recording span reports false from isRecording
--EXTENSIONS--
otel
--ENV--
OTEL_TRACES_EXPORTER=none
--FILE--
<?php
use OpenTelemetry\API\Trace\LocalRootSpan;

var_dump(LocalRootSpan::current()->isRecording());
?>
--EXPECT--
bool(false)
