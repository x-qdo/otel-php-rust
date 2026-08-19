--TEST--
Every declared default value of every native OpenTelemetry method can be read through Reflection
--DESCRIPTION--
Internal functions store default values as PHP source snippets; a malformed
snippet (such as an unquoted empty string) crashes ReflectionParameter::getDefaultValue(),
which breaks reflection-based tooling like the API manifest check.
--EXTENSIONS--
otel
--FILE--
<?php
$checked = 0;
foreach (array_merge(get_declared_classes(), get_declared_interfaces()) as $name) {
    if (!str_starts_with($name, 'OpenTelemetry\\')) {
        continue;
    }
    foreach ((new ReflectionClass($name))->getMethods() as $method) {
        foreach ($method->getParameters() as $parameter) {
            if (!$parameter->isDefaultValueAvailable()) {
                continue;
            }
            $value = $parameter->getDefaultValue();
            $checked++;
            if ($name === 'OpenTelemetry\\API\\Logs\\LogRecord' && $method->getName() === '__construct') {
                var_dump($value);
            }
        }
    }
}
var_dump($checked > 0);
?>
--EXPECT--
string(0) ""
bool(true)
