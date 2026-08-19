//! Panic strategy.
//!
//! The crate is built with `panic = "unwind"` and every `extern "C"` entry
//! point that runs extension code (phper's `invoke`, module/request hooks and
//! object handlers; the observer hooks in `auto`) catches unwinding panics, so
//! an internal bug surfaces as a catchable PHP `\Error` (handlers), a pending
//! `\Error` (object creation) or a logged and swallowed hook failure instead of
//! aborting the PHP-FPM worker or CLI process. This module owns the
//! process-wide panic hook that turns every panic into a rate-limited entry in
//! the extension log, and the helper the extension's own boundaries use.

use std::{
    any::Any,
    cell::Cell,
    panic::{self, AssertUnwindSafe, PanicHookInfo},
    sync::{
        Once,
        atomic::{AtomicU64, Ordering},
    },
};

/// Panics logged with message and location per process before the hook goes
/// quiet. A buggy hot path could otherwise flood the log on every call.
pub const PANIC_LOG_LIMIT: u64 = 10;

static PANIC_COUNT: AtomicU64 = AtomicU64::new(0);
static HOOK: Once = Once::new();

thread_local! {
    static IN_HOOK: Cell<bool> = const { Cell::new(false) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogDecision {
    /// Log message and location.
    Log,
    /// The limit was just reached: log once that further panics are suppressed.
    Announce,
    /// Beyond the limit: count only.
    Silent,
}

/// Decision for the `count`-th panic (1-based) of this process.
pub fn log_decision(count: u64, limit: u64) -> LogDecision {
    if count <= limit {
        LogDecision::Log
    } else if count == limit + 1 {
        LogDecision::Announce
    } else {
        LogDecision::Silent
    }
}

/// Install the panic hook. Idempotent; later calls (after fork, repeated
/// module loads in tests) are no-ops.
pub fn install_hook_once() {
    HOOK.call_once(|| {
        panic::set_hook(Box::new(on_panic));
    });
}

/// Panics raised in this process so far (including contained ones).
pub fn panic_count() -> u64 {
    PANIC_COUNT.load(Ordering::Relaxed)
}

fn on_panic(info: &PanicHookInfo<'_>) {
    // The hook runs before unwinding starts; a panic raised while logging
    // (closed stderr, a logging-layer bug) would re-enter it and recurse.
    if IN_HOOK.with(|flag| flag.replace(true)) {
        return;
    }
    let count = PANIC_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
        record(log_decision(count, PANIC_LOG_LIMIT), info);
    }));
    IN_HOOK.with(|flag| flag.set(false));
}

fn record(decision: LogDecision, info: &PanicHookInfo<'_>) {
    match decision {
        LogDecision::Log => {
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "<unknown>".to_string());
            tracing::error!(
                target: "otel::panic",
                "internal panic contained: {} at {}",
                payload_message(info.payload()),
                location
            );
        }
        LogDecision::Announce => {
            tracing::error!(
                target: "otel::panic",
                "internal panic contained; further panic diagnostics are suppressed for this process (limit {})",
                PANIC_LOG_LIMIT
            );
        }
        LogDecision::Silent => {}
    }
}

/// Text of a panic payload: `panic!("...")` and formatted messages are `&str`
/// or `String`; anything else is reported generically.
pub fn payload_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "non-string panic payload"
    }
}

/// Run `f` at an FFI boundary the extension owns itself (observer hooks).
/// A panic inside `f` is contained and reported through the hook; the caller
/// gets `None` and carries on with engine state untouched.
pub fn contain<T>(f: impl FnOnce() -> T) -> Option<T> {
    panic::catch_unwind(AssertUnwindSafe(f)).ok()
}

/// Test-only panic probes (`cargo build --features test`): PHP-visible
/// functions, classes and lifecycle hooks that panic on demand so the phpt
/// suite can prove each FFI boundary contains a panic.
#[cfg(feature = "test")]
pub mod probes {
    // Panicking on purpose is the whole point of these probes.
    #![allow(clippy::panic)]

    use phper::{
        classes::{ClassEntity, Visibility},
        functions::Argument,
        modules::Module,
        types::ArgumentTypeHint,
    };

    const PROBE_CLASS_NAME: &str = r"OpenTelemetry\Test\PanicProbe";
    const PANIC_STATE_CLASS_NAME: &str = r"OpenTelemetry\Test\PanicState";
    /// Environment variable naming the lifecycle stage (`rinit`, `rshutdown`)
    /// that must panic.
    pub const PANIC_AT_ENV: &str = "OTEL_TEST_PANIC_AT";

    pub fn register(module: &mut Module) {
        module
            .add_function("otel_test_panic", |arguments| {
                let target = crate::util::arg(arguments, 0)?.expect_z_str()?.to_str()?;
                let result: phper::Result<()> = match target {
                    "function" => panic!("test panic in function"),
                    "non-string" => std::panic::panic_any(42_u32),
                    other => Err(phper::Error::boxed(format!("unknown panic target {other:?}"))),
                };
                result
            })
            .argument(Argument::new("where").with_type_hint(ArgumentTypeHint::String));

        let mut probe = ClassEntity::<()>::new(PROBE_CLASS_NAME);
        probe.add_method("panic", Visibility::Public, |_, _| -> phper::Result<()> {
            panic!("test panic in method");
        });
        probe.add_static_method("panicStatic", Visibility::Public, |_| -> phper::Result<()> {
            panic!("test panic in static method");
        });
        module.add_class(probe);

        let panic_state = ClassEntity::<()>::new_with_state_constructor(
            PANIC_STATE_CLASS_NAME,
            || panic!("test panic in state constructor"),
        );
        module.add_class(panic_state);
    }

    /// Panic when `OTEL_TEST_PANIC_AT` names `stage`.
    pub fn panic_at(stage: &str) {
        if std::env::var(PANIC_AT_ENV).is_ok_and(|value| value == stage) {
            panic!("test panic in {stage}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_limit_panics_are_logged_then_one_announcement_then_silence() {
        assert_eq!(log_decision(1, 3), LogDecision::Log);
        assert_eq!(log_decision(3, 3), LogDecision::Log);
        assert_eq!(log_decision(4, 3), LogDecision::Announce);
        assert_eq!(log_decision(5, 3), LogDecision::Silent);
        assert_eq!(log_decision(u64::MAX, 3), LogDecision::Silent);
    }

    #[test]
    fn payload_message_reads_str_and_string_payloads() {
        let from_str: Box<dyn Any + Send> = Box::new("static message");
        assert_eq!(payload_message(from_str.as_ref()), "static message");
        let from_string: Box<dyn Any + Send> = Box::new(String::from("owned message"));
        assert_eq!(payload_message(from_string.as_ref()), "owned message");
        let other: Box<dyn Any + Send> = Box::new(42_u32);
        assert_eq!(payload_message(other.as_ref()), "non-string panic payload");
    }

    #[test]
    fn contain_returns_none_on_panic_and_the_value_otherwise() {
        assert_eq!(contain(|| 7), Some(7));
        let contained = contain(|| -> u32 { panic!("boom") });
        assert_eq!(contained, None);
    }
}
