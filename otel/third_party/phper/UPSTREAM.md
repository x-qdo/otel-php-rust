# Vendored phper

This directory is the `phper` crate from upstream tag `phper-v0.17.5`
(commit `904ba7f94f97207fc33457c6088a5a8af425697d`). `Cargo.toml` replaces the
workspace inheritance with the concrete values of that tag (phper-alloc 0.16.3,
phper-macros 0.15.3, phper-sys/phper-build 0.15.6); `LICENSE` and `README.md`
are copies of the upstream symlink targets.

It is vendored for five local changes. Remove this copy once equivalent fixes
are available in a released upstream crate.

- `create_object` allocated a fresh Zend object-handler table for every PHP
  object. Zend does not own or release that table, so manual span-heavy
  long-running workers grew native RSS linearly even after PHP had destroyed
  the builder and span objects. The local change shares two immutable
  process-lifetime handler tables: one for cloneable state objects and one for
  non-cloneable state objects.
- `invoke` resolves its handler from `zend_internal_function.reserved[slot]`
  (`zend_get_resource_handle("phper")`), populated at MINIT for every module
  function and class method. Upstream 0.17.1 (#220) removed the hidden
  arg_info trailer - PHP rebuilds the arg_info array of every function with a
  declared parameter or return type and dropped it - and now looks the handler
  up by class and function name on every call, which costs two `CString`
  allocations plus a hash lookup per PHP-visible method call. The reserved slot
  survives the arg_info rebuild and `zend_duplicate_internal_function`; the
  name lookup remains as the fallback.
- `find_real_ce` returns immediately for internal classes owned by this module
  and compares class-entry pointers for userland subclasses instead of
  comparing names for every object creation.
- Signature fidelity for the OpenTelemetry API surface: `ClassEntity::set_final`
  / `set_abstract` (class `ce_flags`), `MethodEntity::set_final`,
  `ArgumentTypeHint::Union` / `ReturnTypeHint::Union` (one class member plus
  scalar `MAY_BE_*` members, e.g. `Severity|int`, `float|int`),
  `ArgumentTypeHint::Intersection` / `ReturnTypeHint::Intersection` for PHP
  8.1+, `ReturnTypeHint::Static`, `ArgumentTypeHint::False` for
  `T|false|null` unions, and `Argument::variadic()`
  (`callable ...$callbacks`). PHP 8.1/8.2 cannot normalize a prebuilt
  intersection type list in extension arginfo, so those types are installed
  immediately after class/interface registration; PHP 8.3+ consumes the
  persistent type list directly. Default-value snippets are retained for both
  class-typed and untyped optional parameters (the Zend/phper helpers otherwise
  drop them), so Reflection reports the real default and named arguments can
  skip parameters correctly. Internal enums are marked final and are
  registered before classes/interfaces, allowing enum types to be resolved
  while Zend checks implemented method signatures.

- Panic containment at the FFI boundary. Rust aborts the process when a
  panic unwinds out of an `extern "C"` function, so upstream turns any panic
  in extension code into a dead PHP-FPM worker or CLI process. Every engine
  entry point that runs extension code now wraps it in
  `std::panic::catch_unwind`: `invoke` (`src/functions.rs`) rethrows the
  panic as a catchable PHP `\Error` with message
  `"<module name>: internal error: <panic payload>"` (`PanicError` /
  `throw_panic` in `src/errors.rs`) and returns null; `module_startup`
  (`src/modules.rs`) returns `FAILURE` so PHP refuses to start the module,
  while `request_startup`, `request_shutdown`, `module_shutdown` and
  `module_info` return `SUCCESS` and continue (the engine `exit(1)`s the
  process when RINIT fails, which is what the change avoids); `create_object`
  and `clone_object` (`src/classes.rs`) catch a panicking state constructor or
  cloner, leave the object with a null state and a pending `\Error`;
  `free_object` leaks a state whose `Drop` panics instead of unwinding into
  `zend_object_std_dtor`. `StateObj::drop_state`/`into_state`
  (`src/objects.rs`) tolerate the null state and the accessors name the cause.
  Logging of the panic is left to the process panic hook (the extension
  installs a rate-limited one). `Module::name()` is exposed for the message.

The local source delta from `phper-v0.17.5` is confined to seven files:
`src/classes.rs`, `src/enums.rs`, `src/errors.rs`, `src/functions.rs`,
`src/modules.rs`, `src/objects.rs`, and `src/types.rs`. Formatting-only hunks
may appear in the same files, but no other vendored source file is patched.

To re-vendor: extract `phper/` from the upstream tag, restore the concrete
`Cargo.toml` values and file copies above, and reapply the five change groups
described here to those seven source files. Diff the result against upstream
commit `904ba7f94f97207fc33457c6088a5a8af425697d` before updating this document
or the pinned dependency.
