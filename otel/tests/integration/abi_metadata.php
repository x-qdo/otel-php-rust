<?php
// Prints the ABI/platform metadata recorded next to a release artifact. Run with the
// extension loaded in the exact PHP image the artifact targets.
$extension = new ReflectionExtension('otel');
echo json_encode([
    'extension' => 'otel',
    'extension_version' => $extension->getVersion(),
    'php_version' => PHP_VERSION,
    'php_version_id' => PHP_VERSION_ID,
    // e.g. no-debug-non-zts-20220829: debug flag, thread safety and Zend module API number.
    'zend_module_abi' => basename(PHP_EXTENSION_DIR),
    'thread_safe' => ZEND_THREAD_SAFE,
    'debug_build' => ZEND_DEBUG_BUILD,
    'arch' => php_uname('m'),
    'os' => php_uname('s'),
    'libc' => 'musl (dynamic linkage, libgcc required)',
    'sapi_tested' => PHP_SAPI,
], JSON_PRETTY_PRINT | JSON_THROW_ON_ERROR), "\n";
