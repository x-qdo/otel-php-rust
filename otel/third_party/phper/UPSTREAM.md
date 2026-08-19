# Vendored phper

This directory is the `phper` crate from upstream commit
`84adb0bf37890162df8fe4488a7bd009413e3ee9` (version 0.17.0).

It is vendored because `create_object` allocated a fresh Zend object-handler
table for every PHP object. Zend does not own or release that table, so manual
span-heavy long-running workers grew native RSS linearly even after PHP had
destroyed the builder and span objects.

The local change shares two immutable process-lifetime handler tables: one for
cloneable state objects and one for non-cloneable state objects. Remove this
vendor copy once the equivalent fix is available in a released upstream crate.

Two further local changes remove per-call overhead on the span hot path:

- `invoke` resolves its handler from `zend_internal_function.reserved[slot]`
  (`zend_get_resource_handle("phper")`), populated at MINIT. PHP rebuilds the
  arg_info array of every function with a declared parameter or return type
  (`zend_register_functions`), which drops phper's hidden trailer and previously
  forced a `CString` allocation + `HashMap` lookup on every such call. The trailer
  and the name lookup remain as fallbacks.
- `find_real_ce` returns immediately for internal classes owned by this module and
  compares class-entry pointers for userland subclasses instead of comparing names.

