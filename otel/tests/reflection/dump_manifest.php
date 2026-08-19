<?php
/**
 * Dump a normalised reflection manifest of the OpenTelemetry\API and
 * OpenTelemetry\Context namespaces.
 *
 * Usage:
 *   php dump_manifest.php official <vendor-dir> <out.json>
 *       Loads the Composer-installed open-telemetry/api and open-telemetry/context
 *       packages found under <vendor-dir> and dumps every class, interface, enum,
 *       trait and namespaced function they declare.
 *   php dump_manifest.php extension <official-manifest.json> <out.json>
 *       Dumps the same names as declared by the loaded `otel` extension (no
 *       autoloader, no Composer) plus any extension-declared name in those
 *       namespaces that the official manifest does not know.
 *
 * The two dumps must come from two separate PHP processes so that userland and
 * native declarations never coexist. compare_manifest.php diffs them.
 */
declare(strict_types=1);

const NAMESPACES = ['OpenTelemetry\\API\\', 'OpenTelemetry\\Context\\'];

function in_scope(string $name): bool
{
    foreach (NAMESPACES as $prefix) {
        if (str_starts_with($name, $prefix)) {
            return true;
        }
    }

    return false;
}

function type_string(?ReflectionType $type): ?string
{
    if ($type === null) {
        return null;
    }
    if ($type instanceof ReflectionNamedType) {
        $name = $type->getName();
        if ($name === 'self' || $name === 'static') {
            return $name;
        }
        // Internal classes report "?T" through allowsNull() only; userland may
        // spell it "?T" or "T|null". Normalise both to "?T".
        $nullable = $type->allowsNull() && $name !== 'mixed' && $name !== 'null';

        return ($nullable ? '?' : '') . $name;
    }
    if ($type instanceof ReflectionUnionType || $type instanceof ReflectionIntersectionType) {
        $parts = array_map(static fn (ReflectionType $t) => type_string($t), $type->getTypes());
        $glue = $type instanceof ReflectionUnionType ? '|' : '&';
        sort($parts, SORT_STRING);

        return implode($glue, $parts);
    }

    return (string) $type;
}

function value_repr(mixed $value): string
{
    if (is_array($value)) {
        $parts = [];
        foreach ($value as $k => $v) {
            $parts[] = var_export($k, true) . '=>' . value_repr($v);
        }

        return '[' . implode(',', $parts) . ']';
    }
    if ($value instanceof UnitEnum) {
        return '\\' . $value::class . '::' . $value->name;
    }
    if (is_object($value)) {
        return 'new \\' . $value::class;
    }

    return var_export($value, true);
}

function parameter_entry(ReflectionParameter $p): array
{
    $entry = [
        'name' => $p->getName(),
        'type' => type_string($p->getType()),
        'by_ref' => $p->isPassedByReference(),
        'variadic' => $p->isVariadic(),
        'optional' => $p->isOptional(),
        'default' => null,
    ];
    if ($p->isDefaultValueAvailable()) {
        try {
            $entry['default'] = $p->isDefaultValueConstant()
                ? '\\' . ltrim($p->getDefaultValueConstantName(), '\\')
                : value_repr($p->getDefaultValue());
        } catch (Throwable $e) {
            $entry['default'] = '<unavailable>';
        }
    }

    return $entry;
}

function function_entry(ReflectionFunctionAbstract $f): array
{
    $params = array_map('parameter_entry', $f->getParameters());

    return [
        'params' => $params,
        'return' => type_string($f->getReturnType()),
        'by_ref_return' => $f->returnsReference(),
        'variadic' => $f->isVariadic(),
    ];
}

function class_entry(ReflectionClass $c): array
{
    $kind = 'class';
    if ($c->isInterface()) {
        $kind = 'interface';
    } elseif ($c->isEnum()) {
        $kind = 'enum';
    } elseif ($c->isTrait()) {
        $kind = 'trait';
    }

    $interfaces = $c->getInterfaceNames();
    sort($interfaces, SORT_STRING);

    $constants = [];
    foreach ($c->getReflectionConstants() as $const) {
        if ($const->getDeclaringClass()->getName() !== $c->getName() || $const->isPrivate()) {
            continue;
        }
        $constants[$const->getName()] = [
            'value' => value_repr($const->getValue()),
            'visibility' => $const->isPublic() ? 'public' : ($const->isProtected() ? 'protected' : 'private'),
            'final' => $const->isFinal(),
        ];
    }
    ksort($constants, SORT_STRING);

    $methods = [];
    foreach ($c->getMethods() as $m) {
        // Methods inherited unchanged from a parent class are described on the
        // parent; interface methods re-declared through an implemented
        // interface keep appearing here because the class owns the signature.
        if ($m->getDeclaringClass()->getName() !== $c->getName() && !$c->isInterface()) {
            continue;
        }
        if ($m->isPrivate()) {
            continue;
        }
        $methods[$m->getName()] = function_entry($m) + [
            'static' => $m->isStatic(),
            'abstract' => $m->isAbstract(),
            'final' => $m->isFinal(),
            'visibility' => $m->isPublic() ? 'public' : 'protected',
        ];
    }
    ksort($methods, SORT_STRING);

    $properties = [];
    foreach ($c->getProperties(ReflectionProperty::IS_PUBLIC) as $p) {
        if ($p->getDeclaringClass()->getName() !== $c->getName()) {
            continue;
        }
        $properties[$p->getName()] = [
            'type' => type_string($p->getType()),
            'static' => $p->isStatic(),
            'readonly' => $p->isReadOnly(),
        ];
    }
    ksort($properties, SORT_STRING);

    $cases = [];
    if ($c->isEnum()) {
        foreach ((new ReflectionEnum($c->getName()))->getCases() as $case) {
            $cases[$case->getName()] = $case instanceof ReflectionEnumBackedCase
                ? value_repr($case->getBackingValue())
                : null;
        }
    }

    return [
        'kind' => $kind,
        'final' => $c->isFinal(),
        'abstract' => $c->isAbstract() && !$c->isInterface(),
        'parent' => $c->getParentClass() ? $c->getParentClass()->getName() : null,
        'interfaces' => $interfaces,
        'constants' => (object) $constants,
        'methods' => (object) $methods,
        'properties' => (object) $properties,
        'cases' => (object) $cases,
    ];
}

function official_names(string $vendorDir): array
{
    $roots = [
        'OpenTelemetry\\API\\' => $vendorDir . '/open-telemetry/api',
        'OpenTelemetry\\Context\\' => $vendorDir . '/open-telemetry/context',
    ];
    $names = [];
    foreach ($roots as $prefix => $dir) {
        if (!is_dir($dir)) {
            fwrite(STDERR, "missing package directory {$dir}\n");
            exit(2);
        }
        $iterator = new RecursiveIteratorIterator(new RecursiveDirectoryIterator($dir, FilesystemIterator::SKIP_DOTS));
        foreach ($iterator as $file) {
            if ($file->getExtension() !== 'php') {
                continue;
            }
            $relative = substr($file->getPathname(), strlen($dir) + 1);
            if (basename($relative) === 'functions.php' || str_contains($relative, '/fiber/') || str_contains($relative, 'vendor/')) {
                continue;
            }
            $names[] = $prefix . str_replace('/', '\\', substr($relative, 0, -4));
        }
    }
    sort($names, SORT_STRING);

    return $names;
}

function dump(array $classNames, array $functionNames, array $meta): array
{
    $classes = [];
    foreach ($classNames as $name) {
        if (!class_exists($name, false) && !interface_exists($name, false) && !trait_exists($name, false) && !enum_exists($name, false)) {
            continue;
        }
        $classes[$name] = class_entry(new ReflectionClass($name));
    }
    ksort($classes, SORT_STRING);

    $functions = [];
    foreach ($functionNames as $name) {
        if (!function_exists($name)) {
            continue;
        }
        $functions[$name] = function_entry(new ReflectionFunction($name));
    }
    ksort($functions, SORT_STRING);

    return ['meta' => $meta, 'classes' => (object) $classes, 'functions' => (object) $functions];
}

function scoped_functions(string $bucket): array
{
    return array_values(array_filter(
        array_map(
            // get_defined_functions() lowercases names; keep the canonical spelling via reflection.
            static fn (string $n) => (new ReflectionFunction($n))->getName(),
            get_defined_functions()[$bucket],
        ),
        'in_scope',
    ));
}

$mode = $argv[1] ?? '';
$input = $argv[2] ?? '';
$output = $argv[3] ?? '';
if (!in_array($mode, ['official', 'extension'], true) || $input === '' || $output === '') {
    fwrite(STDERR, "usage: dump_manifest.php official <vendor-dir> <out.json> | extension <official.json> <out.json>\n");
    exit(2);
}

if ($mode === 'official') {
    if (extension_loaded('otel')) {
        fwrite(STDERR, "official dump must run without the otel extension loaded\n");
        exit(2);
    }
    require $input . '/autoload.php';
    $names = official_names($input);
    foreach ($names as $name) {
        // Force autoloading so class_exists(..., false) sees every declaration.
        class_exists($name) || interface_exists($name) || trait_exists($name) || enum_exists($name);
    }
    $packages = [];
    foreach (['api', 'context'] as $package) {
        $composer = json_decode((string) file_get_contents($input . '/composer/installed.json'), true, 512, JSON_THROW_ON_ERROR);
        foreach ($composer['packages'] ?? $composer as $installed) {
            if (($installed['name'] ?? '') === 'open-telemetry/' . $package) {
                $packages['open-telemetry/' . $package] = $installed['version'];
            }
        }
    }
    $manifest = dump($names, scoped_functions('user'), [
        'source' => 'official',
        'php' => PHP_VERSION,
        'packages' => $packages,
    ]);
} else {
    if (!extension_loaded('otel')) {
        fwrite(STDERR, "extension dump requires the otel extension\n");
        exit(2);
    }
    $official = json_decode((string) file_get_contents($input), true, 512, JSON_THROW_ON_ERROR);
    $names = array_keys($official['classes']);
    foreach (array_merge(get_declared_classes(), get_declared_interfaces(), get_declared_traits()) as $declared) {
        if (in_scope($declared) && (new ReflectionClass($declared))->isInternal()) {
            $names[] = $declared;
        }
    }
    $names = array_values(array_unique($names));
    $functionNames = array_values(array_unique(array_merge(array_keys($official['functions']), scoped_functions('internal'))));
    $manifest = dump($names, $functionNames, [
        'source' => 'extension',
        'php' => PHP_VERSION,
        'extension' => phpversion('otel'),
    ]);
}

file_put_contents($output, json_encode($manifest, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n");
fwrite(STDERR, sprintf("%s manifest: %d classes, %d functions -> %s\n", $mode, count((array) $manifest['classes']), count((array) $manifest['functions']), $output));
