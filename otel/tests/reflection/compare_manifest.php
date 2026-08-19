<?php
/**
 * Compare the extension reflection manifest against the official one under the
 * committed policy (policy.json) and fail on any unexplained difference.
 *
 * Usage: php compare_manifest.php <official.json> <extension.json> <policy.json>
 *
 * Policy sections (every official name must appear in exactly one):
 *   match         — the extension must declare the name with an identical public
 *                   signature; per-name "allow" maps a deviation key to a reason.
 *   pending       — known gap, reported as a warning but not yet enforced.
 *   userland_only — the extension must NOT declare it (Composer provides it).
 * extension_only  — extension names outside the official API, each with a reason.
 * packages        — required open-telemetry/api and open-telemetry/context versions.
 */
declare(strict_types=1);

[$officialFile, $extensionFile, $policyFile] = array_slice($argv, 1) + [null, null, null];
if ($officialFile === null || $extensionFile === null || $policyFile === null) {
    fwrite(STDERR, "usage: compare_manifest.php <official.json> <extension.json> <policy.json>\n");
    exit(2);
}

$official = json_decode((string) file_get_contents($officialFile), true, 512, JSON_THROW_ON_ERROR);
$extension = json_decode((string) file_get_contents($extensionFile), true, 512, JSON_THROW_ON_ERROR);
$policy = json_decode((string) file_get_contents($policyFile), true, 512, JSON_THROW_ON_ERROR);

$errors = [];
$warnings = [];

foreach ($policy['packages'] as $package => $version) {
    $actual = $official['meta']['packages'][$package] ?? null;
    if ($actual !== $version) {
        $errors[] = "official manifest was produced from {$package} " . ($actual ?? 'missing') . ", policy requires {$version}";
    }
}

function diff_signature(array $want, array $have, array $allow, string $name, array &$errors): void
{
    $flag = static function (string $key, string $message) use ($allow, $name, &$errors): void {
        if (array_key_exists($key, $allow)) {
            return;
        }
        $errors[] = "{$name}: {$message} (policy key \"{$key}\")";
    };

    foreach (['kind', 'final', 'abstract', 'parent'] as $field) {
        if ($want[$field] !== $have[$field]) {
            $flag($field, "{$field} official=" . json_encode($want[$field]) . ' extension=' . json_encode($have[$field]));
        }
    }
    if ($want['interfaces'] !== $have['interfaces']) {
        $flag('interfaces', 'interfaces official=' . json_encode($want['interfaces']) . ' extension=' . json_encode($have['interfaces']));
    }
    foreach ($want['constants'] as $const => $spec) {
        if (!isset($have['constants'][$const])) {
            $flag("constant:{$const}", "constant {$const} missing");
        } elseif ($have['constants'][$const] !== $spec) {
            $flag("constant:{$const}", "constant {$const} official=" . json_encode($spec) . ' extension=' . json_encode($have['constants'][$const]));
        }
    }
    foreach (array_diff_key($have['constants'], $want['constants']) as $const => $_) {
        $flag("extra_constant:{$const}", "constant {$const} is extension-only");
    }
    foreach ($want['methods'] as $method => $spec) {
        if (!isset($have['methods'][$method])) {
            $flag("method:{$method}", "method {$method} missing");
            continue;
        }
        $got = $have['methods'][$method];
        foreach ($spec as $field => $value) {
            if (($got[$field] ?? null) === $value) {
                continue;
            }
            if ($field === 'params') {
                $flag("method:{$method}:params", "method {$method} params official=" . json_encode($value, JSON_UNESCAPED_SLASHES) . ' extension=' . json_encode($got['params'] ?? null, JSON_UNESCAPED_SLASHES));
            } else {
                $flag("method:{$method}:{$field}", "method {$method} {$field} official=" . json_encode($value) . ' extension=' . json_encode($got[$field] ?? null));
            }
        }
    }
    foreach (array_diff_key($have['methods'], $want['methods']) as $method => $_) {
        $flag("extra_method:{$method}", "method {$method} is extension-only");
    }
    foreach ($want['properties'] as $property => $spec) {
        if (($have['properties'][$property] ?? null) !== $spec) {
            $flag("property:{$property}", "public property {$property} official=" . json_encode($spec) . ' extension=' . json_encode($have['properties'][$property] ?? null));
        }
    }
    foreach (array_diff_key($have['properties'], $want['properties']) as $property => $_) {
        $flag("extra_property:{$property}", "public property {$property} is extension-only");
    }
    if ($want['cases'] !== $have['cases']) {
        $flag('cases', 'enum cases official=' . json_encode($want['cases']) . ' extension=' . json_encode($have['cases']));
    }
}

$classified = [];
foreach (['match', 'pending', 'userland_only'] as $section) {
    foreach ($policy[$section] ?? [] as $name => $_) {
        if (isset($classified[$name])) {
            $errors[] = "policy lists {$name} in both {$classified[$name]} and {$section}";
        }
        $classified[$name] = $section;
    }
}

foreach ($official['classes'] as $name => $want) {
    $section = $classified[$name] ?? null;
    $have = $extension['classes'][$name] ?? null;
    if ($section === null) {
        $errors[] = "{$name} is declared by the official API but not classified in the policy";
        continue;
    }
    if ($section === 'userland_only') {
        if ($have !== null) {
            $errors[] = "{$name} is userland_only but the extension declares it";
        }
        continue;
    }
    if ($have === null) {
        if ($section === 'pending') {
            $warnings[] = "{$name} pending: not declared by the extension";
        } else {
            $errors[] = "{$name} must match but is not declared by the extension";
        }
        continue;
    }
    $scratch = [];
    diff_signature($want, $have, $policy[$section][$name]['allow'] ?? [], $name, $scratch);
    if ($section === 'pending') {
        foreach ($scratch as $line) {
            $warnings[] = "pending: {$line}";
        }
    } else {
        array_push($errors, ...$scratch);
    }
}

foreach ($extension['classes'] as $name => $_) {
    if (isset($official['classes'][$name])) {
        continue;
    }
    if (!isset($policy['extension_only'][$name])) {
        $errors[] = "{$name} is declared by the extension but is neither part of the official API nor listed under extension_only";
    }
}
foreach ($policy['extension_only'] ?? [] as $name => $_) {
    if (isset($official['classes'][$name])) {
        $errors[] = "policy lists {$name} as extension_only but the official API declares it";
    }
}
foreach (array_keys($classified) as $name) {
    if (!isset($official['classes'][$name])) {
        $errors[] = "policy classifies {$name} which the official API does not declare (stale entry)";
    }
}

foreach ($official['functions'] as $name => $spec) {
    $rule = $policy['functions'][$name] ?? null;
    if ($rule === null) {
        $errors[] = "function {$name} is declared by the official API but not classified in the policy";
    } elseif ($rule === 'userland_only' && isset($extension['functions'][$name])) {
        $errors[] = "function {$name} is userland_only but the extension declares it";
    } elseif ($rule === 'match' && (($extension['functions'][$name] ?? null) !== $spec)) {
        $errors[] = "function {$name} differs or is missing";
    }
}
foreach ($extension['functions'] as $name => $_) {
    if (!isset($official['functions'][$name]) && !isset($policy['extension_only_functions'][$name])) {
        $errors[] = "function {$name} is extension-only and not listed in the policy";
    }
}

$matched = count(array_filter($classified, static fn (string $s) => $s === 'match'));
$pendingCount = count(array_filter($classified, static fn (string $s) => $s === 'pending'));
fwrite(STDERR, sprintf(
    "manifest policy: %d official names (%d match, %d pending, %d userland_only), %d extension-only; %d warnings, %d errors\n",
    count($official['classes']),
    $matched,
    $pendingCount,
    count($policy['userland_only'] ?? []),
    count($policy['extension_only'] ?? []),
    count($warnings),
    count($errors),
));
foreach ($warnings as $line) {
    fwrite(STDERR, "warning: {$line}\n");
}
foreach ($errors as $line) {
    fwrite(STDERR, "error: {$line}\n");
}
exit($errors === [] ? 0 : 1);
