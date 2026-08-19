use futures_util::future::join_all;
use opentelemetry::Context;
use opentelemetry_sdk::{
    Resource,
    error::{OTelSdkError, OTelSdkResult},
    trace::{Span, SpanData, SpanExporter, SpanProcessor},
};
use std::{
    collections::hash_map::RandomState,
    env, fmt,
    hash::{BuildHasher, Hasher},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

const DEFAULT_MAX_QUEUE_SIZE: usize = 2_048;
const DEFAULT_MAX_EXPORT_BATCH_SIZE: usize = 512;
const DEFAULT_SCHEDULE_DELAY_MS: u64 = 1_000;
const DEFAULT_EXPORT_TIMEOUT_MS: u64 = 3_000;
const DEFAULT_SHUTDOWN_TIMEOUT_MS: u64 = 500;

const MAX_QUEUE_SIZE: usize = 65_536;
const MAX_EXPORT_BATCH_SIZE: usize = 4_096;
const MAX_CONCURRENT_EXPORTS: usize = 8;
const MAX_SCHEDULE_DELAY_MS: u64 = 60_000;
const MAX_EXPORT_TIMEOUT_MS: u64 = 30_000;
const MAX_SHUTDOWN_TIMEOUT_MS: u64 = 2_000;

const DEFAULT_RETRY_MAX_ATTEMPTS: usize = 3;
const MAX_RETRY_MAX_ATTEMPTS: usize = 10;
const DEFAULT_RETRY_MAX_ELAPSED_MS: u64 = 5_000;
const MAX_RETRY_MAX_ELAPSED_MS: u64 = 30_000;
const DEFAULT_RETRY_INITIAL_BACKOFF_MS: u64 = 100;
const MAX_RETRY_INITIAL_BACKOFF_MS: u64 = 5_000;
const RETRY_BACKOFF_CAP: Duration = Duration::from_millis(5_000);
/// Longest uninterrupted sleep inside a retry backoff; the worker re-checks the
/// abort and flush flags between slices.
const RETRY_BACKOFF_SLICE: Duration = Duration::from_millis(10);

/// Bounded transient-retry policy applied per export batch on the worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts per batch including the first; 1 disables retries.
    pub max_attempts: usize,
    /// Wall-clock budget per batch, counted from the first attempt. A retry
    /// starts only while budget remains; the attempt itself is bounded by the
    /// exporter timeout.
    pub max_elapsed: Duration,
    pub initial_backoff: Duration,
}

impl RetryPolicy {
    fn from_env() -> Result<Self, String> {
        let max_attempts = parse_usize(
            "OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS",
            DEFAULT_RETRY_MAX_ATTEMPTS,
            MAX_RETRY_MAX_ATTEMPTS,
        )?;
        let max_elapsed_ms = parse_u64_range(
            "OTEL_PHP_EXPORT_RETRY_MAX_ELAPSED",
            DEFAULT_RETRY_MAX_ELAPSED_MS,
            0,
            MAX_RETRY_MAX_ELAPSED_MS,
        )?;
        let initial_backoff_ms = parse_u64(
            "OTEL_PHP_EXPORT_RETRY_INITIAL_BACKOFF",
            DEFAULT_RETRY_INITIAL_BACKOFF_MS,
            MAX_RETRY_INITIAL_BACKOFF_MS,
        )?;
        Ok(Self {
            max_attempts,
            max_elapsed: Duration::from_millis(max_elapsed_ms),
            initial_backoff: Duration::from_millis(initial_backoff_ms),
        })
    }

    fn retries_enabled(&self) -> bool {
        self.max_attempts > 1 && !self.max_elapsed.is_zero()
    }

    #[cfg(test)]
    const fn disabled() -> Self {
        Self {
            max_attempts: 1,
            max_elapsed: Duration::ZERO,
            initial_backoff: Duration::from_millis(1),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BatchProcessorConfig {
    pub max_queue_size: usize,
    pub max_export_batch_size: usize,
    /// Batches the worker keeps in flight at once. Only the async gRPC
    /// transport actually overlaps requests; the blocking HTTP client
    /// serialises them.
    pub max_concurrent_exports: usize,
    pub scheduled_delay: Duration,
    pub export_timeout: Duration,
    pub shutdown_timeout: Duration,
    pub retry: RetryPolicy,
}

impl BatchProcessorConfig {
    pub fn from_env() -> Result<Self, String> {
        let max_queue_size = parse_usize(
            "OTEL_BSP_MAX_QUEUE_SIZE",
            DEFAULT_MAX_QUEUE_SIZE,
            MAX_QUEUE_SIZE,
        )?;
        let max_export_batch_size = parse_usize(
            "OTEL_BSP_MAX_EXPORT_BATCH_SIZE",
            DEFAULT_MAX_EXPORT_BATCH_SIZE,
            MAX_EXPORT_BATCH_SIZE,
        )?;
        if max_export_batch_size > max_queue_size {
            return Err(
                "OTEL_BSP_MAX_EXPORT_BATCH_SIZE must not exceed OTEL_BSP_MAX_QUEUE_SIZE"
                    .to_string(),
            );
        }

        let max_concurrent_exports = parse_usize(
            "OTEL_BSP_MAX_CONCURRENT_EXPORTS",
            1,
            MAX_CONCURRENT_EXPORTS,
        )?;

        let scheduled_delay_ms = parse_u64(
            "OTEL_BSP_SCHEDULE_DELAY",
            DEFAULT_SCHEDULE_DELAY_MS,
            MAX_SCHEDULE_DELAY_MS,
        )?;
        let export_timeout_ms = parse_first_u64(
            &[
                "OTEL_EXPORTER_OTLP_TRACES_TIMEOUT",
                "OTEL_EXPORTER_OTLP_TIMEOUT",
            ],
            DEFAULT_EXPORT_TIMEOUT_MS,
            MAX_EXPORT_TIMEOUT_MS,
        )?;
        let shutdown_timeout_ms = parse_u64(
            "OTEL_PHP_SHUTDOWN_TIMEOUT",
            DEFAULT_SHUTDOWN_TIMEOUT_MS,
            MAX_SHUTDOWN_TIMEOUT_MS,
        )?;
        let retry = RetryPolicy::from_env()?;

        Ok(Self {
            max_queue_size,
            max_export_batch_size,
            max_concurrent_exports,
            scheduled_delay: Duration::from_millis(scheduled_delay_ms),
            export_timeout: Duration::from_millis(export_timeout_ms),
            shutdown_timeout: Duration::from_millis(shutdown_timeout_ms),
            retry,
        })
    }
}

fn parse_usize(name: &str, default: usize, maximum: usize) -> Result<usize, String> {
    match env::var(name) {
        Ok(value) => value
            .parse::<usize>()
            .ok()
            .filter(|value| *value > 0 && *value <= maximum)
            .ok_or_else(|| format!("{name} must be an integer between 1 and {maximum}")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{name} must be valid UTF-8")),
    }
}

fn parse_u64(name: &str, default: u64, maximum: u64) -> Result<u64, String> {
    parse_u64_range(name, default, 1, maximum)
}

fn parse_u64_range(name: &str, default: u64, minimum: u64, maximum: u64) -> Result<u64, String> {
    match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .ok()
            .filter(|value| *value >= minimum && *value <= maximum)
            .ok_or_else(|| format!("{name} must be an integer between {minimum} and {maximum}")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{name} must be valid UTF-8")),
    }
}

fn parse_first_u64(names: &[&str], default: u64, maximum: u64) -> Result<u64, String> {
    for name in names {
        match env::var(name) {
            Ok(value) => {
                return value
                    .parse::<u64>()
                    .ok()
                    .filter(|value| *value > 0 && *value <= maximum)
                    .ok_or_else(|| format!("{name} must be an integer between 1 and {maximum}"));
            }
            Err(env::VarError::NotPresent) => {}
            Err(env::VarError::NotUnicode(_)) => {
                return Err(format!("{name} must be valid UTF-8"));
            }
        }
    }
    Ok(default)
}

#[derive(Clone, Debug, Default)]
pub struct BatchMetrics {
    inner: Arc<BatchMetricsInner>,
}

#[derive(Debug, Default)]
struct BatchMetricsInner {
    sampled_started: AtomicUsize,
    sampled_ended: AtomicUsize,
    queued: AtomicUsize,
    exported: AtomicUsize,
    dropped_queue_full: AtomicUsize,
    dropped_export_failure: AtomicUsize,
    dropped_shutdown: AtomicUsize,
    export_failures: AtomicUsize,
    export_retries: AtomicUsize,
    export_retry_recovered: AtomicUsize,
    queue_depth: AtomicUsize,
    queue_high_watermark: AtomicUsize,
    in_flight: AtomicUsize,
    queue_full_log_emitted: AtomicBool,
    export_failure_log_emitted: AtomicBool,
}

#[derive(Clone, Debug, Default)]
pub struct BatchMetricsSnapshot {
    pub sampled_started: usize,
    pub sampled_ended: usize,
    pub queued: usize,
    pub exported: usize,
    pub dropped_queue_full: usize,
    pub dropped_export_failure: usize,
    pub dropped_shutdown: usize,
    pub export_failures: usize,
    /// Retry attempts performed (attempts beyond the first, summed over batches).
    pub export_retries: usize,
    /// Batches that were exported after at least one retry.
    pub export_retry_recovered: usize,
    pub queue_depth: usize,
    pub queue_high_watermark: usize,
    pub in_flight: usize,
}

impl BatchMetrics {
    pub fn snapshot(&self) -> BatchMetricsSnapshot {
        let inner = &self.inner;
        BatchMetricsSnapshot {
            sampled_started: inner.sampled_started.load(Ordering::Relaxed),
            sampled_ended: inner.sampled_ended.load(Ordering::Relaxed),
            queued: inner.queued.load(Ordering::Relaxed),
            exported: inner.exported.load(Ordering::Relaxed),
            dropped_queue_full: inner.dropped_queue_full.load(Ordering::Relaxed),
            dropped_export_failure: inner.dropped_export_failure.load(Ordering::Relaxed),
            dropped_shutdown: inner.dropped_shutdown.load(Ordering::Relaxed),
            export_failures: inner.export_failures.load(Ordering::Relaxed),
            export_retries: inner.export_retries.load(Ordering::Relaxed),
            export_retry_recovered: inner.export_retry_recovered.load(Ordering::Relaxed),
            queue_depth: inner.queue_depth.load(Ordering::Relaxed),
            queue_high_watermark: inner.queue_high_watermark.load(Ordering::Relaxed),
            in_flight: inner.in_flight.load(Ordering::Relaxed),
        }
    }

    fn update_high_watermark(&self, depth: usize) {
        self.inner
            .queue_high_watermark
            .fetch_max(depth, Ordering::Relaxed);
    }
}

/// Whether a failed export attempt may be retried under the retry policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportErrorKind {
    Retryable,
    Terminal,
}

/// Map an exporter failure onto the OTLP retry guidance. Retryable: gRPC
/// CANCELLED, DEADLINE_EXCEEDED, ABORTED, OUT_OF_RANGE, UNAVAILABLE, DATA_LOSS,
/// RESOURCE_EXHAUSTED; HTTP 429, 502, 503, 504; connect/DNS/transport/timeout
/// failures. Everything else (INVALID_ARGUMENT, UNAUTHENTICATED,
/// PERMISSION_DENIED, UNIMPLEMENTED, INTERNAL, NOT_FOUND, HTTP 4xx/5xx other
/// than the list above, exporter-internal errors) is terminal.
///
/// opentelemetry-otlp 0.31 flattens every transport error into
/// `OTelSdkError::InternalFailure(String)`, so the decision is made on the
/// message shapes produced by tonic (`code: '<description>', message: ...`),
/// the reqwest blocking client (`reqwest::Error { kind: ..., url: ..., source: ... }`),
/// and the exporter's own HTTP status line (`Status Code: NNN`).
pub fn classify_export_error(error: &OTelSdkError) -> ExportErrorKind {
    match error {
        OTelSdkError::Timeout(_) => ExportErrorKind::Retryable,
        OTelSdkError::AlreadyShutdown => ExportErrorKind::Terminal,
        OTelSdkError::InternalFailure(message) => classify_export_message(message),
    }
}

fn classify_export_message(message: &str) -> ExportErrorKind {
    if let Some(kind) = classify_grpc_status(message) {
        return kind;
    }
    if let Some(status) = http_status_code(message) {
        return match status {
            429 | 502 | 503 | 504 => ExportErrorKind::Retryable,
            _ => ExportErrorKind::Terminal,
        };
    }
    if message.starts_with("reqwest::Error { kind: Request") || is_transport_failure(message) {
        ExportErrorKind::Retryable
    } else {
        ExportErrorKind::Terminal
    }
}

/// tonic's `Status` Display is `code: '<Code description>'[, message: ...][, source: ...]`;
/// older releases and Debug output use the variant name instead. Recognised
/// codes decide on their own; UNKNOWN defers to the transport heuristics.
fn classify_grpc_status(message: &str) -> Option<ExportErrorKind> {
    const RETRYABLE: [&str; 14] = [
        "The operation was cancelled",
        "Cancelled",
        "Deadline expired before operation could complete",
        "DeadlineExceeded",
        "The operation was aborted",
        "Aborted",
        "Operation was attempted past the valid range",
        "OutOfRange",
        "The service is currently unavailable",
        "Unavailable",
        "Unrecoverable data loss or corruption",
        "DataLoss",
        "Some resource has been exhausted",
        "ResourceExhausted",
    ];
    const UNKNOWN: [&str; 2] = ["Unknown error", "Unknown"];

    let code = message
        .strip_prefix("code: ")
        .or_else(|| message.strip_prefix("status: "))?;
    let code = code.trim_start_matches('\'');
    let code_end = code
        .find(['\'', ','])
        .unwrap_or(code.len());
    let code = &code[..code_end];
    if RETRYABLE.contains(&code) {
        Some(ExportErrorKind::Retryable)
    } else if UNKNOWN.contains(&code) {
        Some(if is_transport_failure(message) {
            ExportErrorKind::Retryable
        } else {
            ExportErrorKind::Terminal
        })
    } else {
        Some(ExportErrorKind::Terminal)
    }
}

fn http_status_code(message: &str) -> Option<u16> {
    let digits = |rest: &str| {
        rest.trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u16>()
            .ok()
    };
    if let Some(index) = message.find("kind: Status(") {
        return digits(&message[index + "kind: Status(".len()..]);
    }
    if let Some(index) = message.find("Status Code: ") {
        return digits(&message[index + "Status Code: ".len()..]);
    }
    None
}

fn is_transport_failure(message: &str) -> bool {
    const MARKERS: [&str; 20] = [
        "transport error",
        "error trying to connect",
        "tcp connect error",
        "connection refused",
        "ConnectionRefused",
        "connection reset",
        "ConnectionReset",
        "connection aborted",
        "ConnectionAborted",
        "broken pipe",
        "BrokenPipe",
        "dns error",
        "failed to lookup address",
        "timed out",
        "TimedOut",
        "Timeout expired",
        "deadline",
        "connection closed",
        "channel closed",
        "operation was canceled",
    ];
    MARKERS.iter().any(|marker| message.contains(marker))
}

/// Flags the request side raises for the worker; polled between backoff slices.
#[derive(Clone, Debug, Default)]
struct WorkerSignals {
    /// Set when the shutdown budget expired; the worker abandons the current
    /// round without accounting (shutdown already counted it as dropped_shutdown).
    abort_export: Arc<AtomicBool>,
    /// Set while a force_flush/shutdown is waiting; cuts retry backoffs short.
    flush_requested: Arc<AtomicBool>,
}

#[derive(Debug)]
enum ControlMessage {
    Export,
    ForceFlush(SyncSender<OTelSdkResult>),
    Shutdown(SyncSender<OTelSdkResult>),
    SetResource(Arc<Resource>),
}

pub struct BoundedBatchSpanProcessor {
    // Boxed so the bounded ring buffer holds pointers instead of ~400-byte
    // SpanData slots: the request thread hands off one pointer-sized value and
    // the ring buffer stays a few KiB instead of max_queue_size * sizeof(SpanData).
    span_sender: SyncSender<Box<SpanData>>,
    control_sender: SyncSender<ControlMessage>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
    metrics: BatchMetrics,
    export_pending: Arc<AtomicBool>,
    signals: WorkerSignals,
    shutdown_started: AtomicBool,
    config: BatchProcessorConfig,
}

impl fmt::Debug for BoundedBatchSpanProcessor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundedBatchSpanProcessor")
            .field("metrics", &self.metrics.snapshot())
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl BoundedBatchSpanProcessor {
    pub fn new<E>(
        exporter: E,
        config: BatchProcessorConfig,
        metrics: BatchMetrics,
    ) -> Result<Self, String>
    where
        E: SpanExporter + 'static,
    {
        let (span_sender, span_receiver) = mpsc::sync_channel::<Box<SpanData>>(config.max_queue_size);
        let (control_sender, control_receiver) = mpsc::sync_channel(8);
        let export_pending = Arc::new(AtomicBool::new(false));
        let signals = WorkerSignals::default();

        let worker_config = config.clone();
        let worker_metrics = metrics.clone();
        let worker_export_pending = export_pending.clone();
        let worker_signals = signals.clone();
        let handle = thread::Builder::new()
            .name("otel-php-trace-export".to_string())
            .spawn(move || {
                run_worker(
                    exporter,
                    span_receiver,
                    control_receiver,
                    worker_config,
                    worker_metrics,
                    worker_export_pending,
                    worker_signals,
                )
            })
            .map_err(|error| format!("failed to create trace exporter worker: {error}"))?;

        Ok(Self {
            span_sender,
            control_sender,
            handle: Mutex::new(Some(handle)),
            metrics,
            export_pending,
            signals,
            shutdown_started: AtomicBool::new(false),
            config,
        })
    }

    fn notify_export(&self) {
        if !self.export_pending.swap(true, Ordering::Relaxed)
            && self
                .control_sender
                .try_send(ControlMessage::Export)
                .is_err()
        {
            self.export_pending.store(false, Ordering::Relaxed);
        }
    }

    fn wait_for_control(
        &self,
        message: ControlMessage,
        receiver: Receiver<OTelSdkResult>,
        timeout: Duration,
    ) -> OTelSdkResult {
        self.control_sender
            .try_send(message)
            .map_err(|error| match error {
                TrySendError::Full(_) => OTelSdkError::InternalFailure(
                    "trace exporter control queue is full".to_string(),
                ),
                TrySendError::Disconnected(_) => OTelSdkError::AlreadyShutdown,
            })?;

        receiver
            .recv_timeout(timeout)
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => OTelSdkError::Timeout(timeout),
                RecvTimeoutError::Disconnected => {
                    OTelSdkError::InternalFailure("trace exporter worker stopped".to_string())
                }
            })?
    }
}

impl SpanProcessor for BoundedBatchSpanProcessor {
    fn on_start(&self, _span: &mut Span, _context: &Context) {
        self.metrics
            .inner
            .sampled_started
            .fetch_add(1, Ordering::Relaxed);
    }

    fn on_end(&self, span: SpanData) {
        self.metrics
            .inner
            .sampled_ended
            .fetch_add(1, Ordering::Relaxed);

        if self.shutdown_started.load(Ordering::Relaxed) {
            self.metrics
                .inner
                .dropped_shutdown
                .fetch_add(1, Ordering::Relaxed);
            return;
        }

        let depth = self
            .metrics
            .inner
            .queue_depth
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        match self.span_sender.try_send(Box::new(span)) {
            Ok(()) => {
                self.metrics.inner.queued.fetch_add(1, Ordering::Relaxed);
                // The worker can dequeue between our pre-increment and the
                // successful try_send, making this local value one above the
                // channel capacity even though the channel itself is bounded.
                self.metrics
                    .update_high_watermark(depth.min(self.config.max_queue_size));
                if depth >= self.config.max_export_batch_size {
                    self.notify_export();
                }
            }
            Err(TrySendError::Full(_)) => {
                self.metrics
                    .inner
                    .queue_depth
                    .fetch_sub(1, Ordering::Relaxed);
                self.metrics
                    .inner
                    .dropped_queue_full
                    .fetch_add(1, Ordering::Relaxed);
                if !self
                    .metrics
                    .inner
                    .queue_full_log_emitted
                    .swap(true, Ordering::Relaxed)
                {
                    tracing::warn!(
                        "BoundedBatchProcessor.QueueFull dropping spans; further queue-full diagnostics are suppressed until shutdown"
                    );
                }
            }
            Err(TrySendError::Disconnected(_)) => {
                self.metrics
                    .inner
                    .queue_depth
                    .fetch_sub(1, Ordering::Relaxed);
                self.metrics
                    .inner
                    .dropped_shutdown
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn force_flush(&self) -> OTelSdkResult {
        self.signals.flush_requested.store(true, Ordering::SeqCst);
        let (sender, receiver) = mpsc::sync_channel(1);
        self.wait_for_control(
            ControlMessage::ForceFlush(sender),
            receiver,
            self.config.shutdown_timeout,
        )
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        if self.shutdown_started.swap(true, Ordering::SeqCst) {
            return Err(OTelSdkError::AlreadyShutdown);
        }

        let effective_timeout = timeout.min(self.config.shutdown_timeout);
        self.signals.flush_requested.store(true, Ordering::SeqCst);
        let (sender, receiver) = mpsc::sync_channel(1);
        let result = self.wait_for_control(
            ControlMessage::Shutdown(sender),
            receiver,
            effective_timeout,
        );

        if matches!(result, Err(OTelSdkError::Timeout(_))) {
            self.signals.abort_export.store(true, Ordering::SeqCst);
            let remaining = self.metrics.inner.queue_depth.load(Ordering::Relaxed)
                + self.metrics.inner.in_flight.load(Ordering::Relaxed);
            self.metrics
                .inner
                .dropped_shutdown
                .fetch_add(remaining, Ordering::Relaxed);
        } else if result.is_ok() {
            let handle = self.handle.lock().ok().and_then(|mut handle| handle.take());
            if let Some(handle) = handle
                && handle.join().is_err() {
                    return Err(OTelSdkError::InternalFailure(
                        "trace exporter worker panicked".to_string(),
                    ));
                }
        }

        result
    }

    fn set_resource(&mut self, resource: &Resource) {
        let _ = self
            .control_sender
            .try_send(ControlMessage::SetResource(Arc::new(resource.clone())));
    }
}

#[allow(clippy::too_many_arguments)]
fn run_worker<E>(
    mut exporter: E,
    span_receiver: Receiver<Box<SpanData>>,
    control_receiver: Receiver<ControlMessage>,
    config: BatchProcessorConfig,
    metrics: BatchMetrics,
    export_pending: Arc<AtomicBool>,
    signals: WorkerSignals,
) where
    E: SpanExporter + 'static,
{
    let _suppression = Context::enter_telemetry_suppressed_scope();
    let mut last_export = Instant::now();
    tracing::debug!(
        "BoundedBatchProcessor.ThreadStarted schedule_delay_ms={} max_export_batch_size={} max_queue_size={}",
        config.scheduled_delay.as_millis(),
        config.max_export_batch_size,
        config.max_queue_size,
    );

    loop {
        let wait = config
            .scheduled_delay
            .checked_sub(last_export.elapsed())
            .unwrap_or(Duration::ZERO);
        match control_receiver.recv_timeout(wait) {
            Ok(ControlMessage::Export) => {
                export_pending.store(false, Ordering::Relaxed);
                let _ = export_available(&exporter, &span_receiver, &config, &metrics, &signals);
                last_export = Instant::now();
            }
            Ok(ControlMessage::ForceFlush(sender)) => {
                tracing::debug!("BoundedBatchProcessor.ExportingDueToForceFlush");
                let result =
                    export_available(&exporter, &span_receiver, &config, &metrics, &signals)
                        .and_then(|_| exporter.force_flush());
                signals.flush_requested.store(false, Ordering::SeqCst);
                let _ = sender.send(result);
                last_export = Instant::now();
            }
            Ok(ControlMessage::Shutdown(sender)) => {
                tracing::debug!("BoundedBatchProcessor.ExportingDueToShutdown");
                let result =
                    export_available(&exporter, &span_receiver, &config, &metrics, &signals)
                        .and_then(|_| exporter.shutdown_with_timeout(config.export_timeout));
                if signals.abort_export.load(Ordering::SeqCst) {
                    discard_remaining(&span_receiver, &metrics);
                }
                let _ = sender.send(result);
                tracing::debug!("BoundedBatchProcessor.ThreadStopped");
                break;
            }
            Ok(ControlMessage::SetResource(resource)) => exporter.set_resource(&resource),
            Err(RecvTimeoutError::Timeout) => {
                let _ = export_available(&exporter, &span_receiver, &config, &metrics, &signals);
                last_export = Instant::now();
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn export_available<E>(
    exporter: &E,
    span_receiver: &Receiver<Box<SpanData>>,
    config: &BatchProcessorConfig,
    metrics: &BatchMetrics,
    signals: &WorkerSignals,
) -> OTelSdkResult
where
    E: SpanExporter + 'static,
{
    let target = metrics.inner.queue_depth.load(Ordering::Relaxed);
    let mut processed = 0;
    let mut first_error = None;

    while processed < target && !signals.abort_export.load(Ordering::SeqCst) {
        let mut batches = Vec::with_capacity(config.max_concurrent_exports);
        while batches.len() < config.max_concurrent_exports {
            let batch = take_batch(span_receiver, config, metrics);
            if batch.is_empty() {
                break;
            }
            batches.push(batch);
        }
        if batches.is_empty() {
            break;
        }
        processed += batches.iter().map(Vec::len).sum::<usize>();

        if let Err(error) = export_round(exporter, batches, config, metrics, signals) {
            first_error.get_or_insert(error);
        }
    }

    if let Some(error) = first_error {
        Err(OTelSdkError::InternalFailure(error))
    } else {
        Ok(())
    }
}

/// Export one set of concurrent batches. Retryable failures back off together
/// and are re-exported concurrently until each batch has succeeded, failed
/// terminally, or exhausted the attempt/elapsed budget. Returns the first
/// terminal error message.
fn export_round<E>(
    exporter: &E,
    mut pending: Vec<Vec<SpanData>>,
    config: &BatchProcessorConfig,
    metrics: &BatchMetrics,
    signals: &WorkerSignals,
) -> Result<(), String>
where
    E: SpanExporter + 'static,
{
    let policy = &config.retry;
    let started = Instant::now();
    let mut attempt = 0usize;
    let mut backoff = policy.initial_backoff;
    let mut first_error: Option<String> = None;

    loop {
        attempt += 1;
        if attempt > 1 {
            metrics
                .inner
                .export_retries
                .fetch_add(pending.len(), Ordering::Relaxed);
        }
        let sizes: Vec<usize> = pending.iter().map(Vec::len).collect();
        metrics
            .inner
            .in_flight
            .store(sizes.iter().sum(), Ordering::Relaxed);

        // Batches are cloned only while a further attempt is still permitted;
        // the final permitted attempt hands them to the exporter by value.
        let may_retry_again = policy.retries_enabled()
            && attempt < policy.max_attempts
            && started.elapsed() < policy.max_elapsed;
        let results = if may_retry_again {
            futures_executor::block_on(join_all(
                pending.iter().map(|batch| exporter.export(batch.clone())),
            ))
        } else {
            futures_executor::block_on(join_all(
                std::mem::take(&mut pending)
                    .into_iter()
                    .map(|batch| exporter.export(batch)),
            ))
        };

        if signals.abort_export.load(Ordering::SeqCst) {
            // The shutdown budget expired: shutdown_with_timeout has already
            // counted everything in flight as dropped_shutdown, so the results
            // of this attempt are not accounted again.
            metrics.inner.in_flight.store(0, Ordering::Relaxed);
            return first_error.map_or(Ok(()), Err);
        }

        let mut retry = Vec::new();
        for (index, (result, batch_size)) in results.into_iter().zip(sizes).enumerate() {
            match result {
                Ok(()) => {
                    metrics
                        .inner
                        .exported
                        .fetch_add(batch_size, Ordering::Relaxed);
                    if attempt > 1 {
                        metrics
                            .inner
                            .export_retry_recovered
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(error) => {
                    let retryable = classify_export_error(&error) == ExportErrorKind::Retryable;
                    tracing::debug!(
                        "BoundedBatchProcessor.ExportAttemptFailed attempt={} max_attempts={} spans={} retryable={} error={}",
                        attempt,
                        policy.max_attempts,
                        batch_size,
                        retryable,
                        error,
                    );
                    if may_retry_again && retryable {
                        // `pending` still holds every batch of this attempt
                        // (they were cloned for the export), so the slot exists.
                        let batch = pending.get_mut(index).map(std::mem::take).unwrap_or_default();
                        retry.push((batch, error.to_string()));
                    } else {
                        record_terminal_failure(
                            metrics,
                            batch_size,
                            &error.to_string(),
                            &mut first_error,
                        );
                    }
                }
            }
        }
        if retry.is_empty() {
            break;
        }

        let remaining = policy.max_elapsed.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            for (batch, error) in retry {
                record_terminal_failure(metrics, batch.len(), &error, &mut first_error);
            }
            break;
        }
        let delay = jittered(backoff).min(RETRY_BACKOFF_CAP).min(remaining);
        backoff = backoff.saturating_mul(2).min(RETRY_BACKOFF_CAP);
        pending = retry.into_iter().map(|(batch, _)| batch).collect();
        let retry_spans: usize = pending.iter().map(Vec::len).sum();
        metrics
            .inner
            .in_flight
            .store(retry_spans, Ordering::Relaxed);
        tracing::debug!(
            "BoundedBatchProcessor.ExportRetryScheduled attempt={} max_attempts={} batches={} spans={} delay_ms={} elapsed_ms={}",
            attempt + 1,
            policy.max_attempts,
            pending.len(),
            retry_spans,
            delay.as_millis(),
            started.elapsed().as_millis(),
        );
        if wait_backoff(delay, signals) {
            // Aborted by the shutdown budget; accounted as dropped_shutdown by
            // shutdown_with_timeout, which read in_flight before setting the flag.
            metrics.inner.in_flight.store(0, Ordering::Relaxed);
            return first_error.map_or(Ok(()), Err);
        }
    }

    metrics.inner.in_flight.store(0, Ordering::Relaxed);
    first_error.map_or(Ok(()), Err)
}

fn record_terminal_failure(
    metrics: &BatchMetrics,
    batch_size: usize,
    error: &str,
    first_error: &mut Option<String>,
) {
    metrics
        .inner
        .export_failures
        .fetch_add(1, Ordering::Relaxed);
    metrics
        .inner
        .dropped_export_failure
        .fetch_add(batch_size, Ordering::Relaxed);
    if !metrics
        .inner
        .export_failure_log_emitted
        .swap(true, Ordering::Relaxed)
    {
        tracing::error!(
            "BoundedBatchProcessor.ExportError; further exporter failure diagnostics are suppressed until shutdown"
        );
    }
    if first_error.is_none() {
        *first_error = Some(error.to_string());
    }
}

/// +/-20 % jitter from a hash-seeded value, so no RNG dependency is needed.
fn jittered(backoff: Duration) -> Duration {
    let random = RandomState::new().build_hasher().finish();
    let percent = 80 + (random % 41) as u32;
    backoff * percent / 100
}

/// Sleep in short slices so the worker notices a shutdown abort (returns true)
/// or an explicit flush/shutdown request (returns false early, so the remaining
/// attempts run inside the caller's wait budget instead of after the backoff).
fn wait_backoff(delay: Duration, signals: &WorkerSignals) -> bool {
    let deadline = Instant::now() + delay;
    loop {
        if signals.abort_export.load(Ordering::SeqCst) {
            return true;
        }
        if signals.flush_requested.load(Ordering::SeqCst) {
            return false;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        thread::sleep(remaining.min(RETRY_BACKOFF_SLICE));
    }
}

fn take_batch(
    span_receiver: &Receiver<Box<SpanData>>,
    config: &BatchProcessorConfig,
    metrics: &BatchMetrics,
) -> Vec<SpanData> {
    let mut batch = Vec::with_capacity(config.max_export_batch_size);
    while batch.len() < config.max_export_batch_size {
        match span_receiver.try_recv() {
            Ok(span) => {
                metrics.inner.queue_depth.fetch_sub(1, Ordering::Relaxed);
                batch.push(*span);
            }
            Err(_) => break,
        }
    }
    batch
}

fn discard_remaining(span_receiver: &Receiver<Box<SpanData>>, metrics: &BatchMetrics) {
    while span_receiver.try_recv().is_ok() {}
    metrics.inner.queue_depth.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_sdk::testing::trace::new_test_export_span_data;
    use std::{
        future::Future,
        pin::Pin,
        task::{Context as TaskContext, Poll},
    };

    /// Timer that completes on a helper thread so several exports can be in
    /// flight without a tokio runtime.
    struct Delay {
        deadline: Instant,
        armed: bool,
    }

    impl Future for Delay {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<()> {
            if Instant::now() >= self.deadline {
                return Poll::Ready(());
            }
            if !self.armed {
                self.armed = true;
                let waker = cx.waker().clone();
                let remaining = self.deadline.saturating_duration_since(Instant::now());
                thread::spawn(move || {
                    thread::sleep(remaining);
                    waker.wake();
                });
            }
            Poll::Pending
        }
    }

    #[derive(Debug, Clone)]
    struct SlowExporter {
        delay: Duration,
        in_flight: Arc<AtomicUsize>,
        max_in_flight: Arc<AtomicUsize>,
    }

    impl SpanExporter for SlowExporter {
        fn export(&self, _batch: Vec<SpanData>) -> impl Future<Output = OTelSdkResult> + Send {
            let in_flight = self.in_flight.clone();
            let max_in_flight = self.max_in_flight.clone();
            let delay = self.delay;
            async move {
                let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                max_in_flight.fetch_max(now, Ordering::SeqCst);
                Delay {
                    deadline: Instant::now() + delay,
                    armed: false,
                }
                .await;
                in_flight.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            }
        }
    }

    #[test]
    fn concurrent_exports_overlap_up_to_the_configured_limit() {
        let exporter = SlowExporter {
            delay: Duration::from_millis(150),
            in_flight: Arc::new(AtomicUsize::new(0)),
            max_in_flight: Arc::new(AtomicUsize::new(0)),
        };
        let max_in_flight = exporter.max_in_flight.clone();
        let config = BatchProcessorConfig {
            max_queue_size: 64,
            max_export_batch_size: 4,
            max_concurrent_exports: 3,
            scheduled_delay: Duration::from_secs(60),
            export_timeout: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(2),
            retry: RetryPolicy::disabled(),
        };
        let metrics = BatchMetrics::default();
        let processor = BoundedBatchSpanProcessor::new(exporter, config, metrics.clone())
            .expect("processor should start");

        // Three full batches, but the export trigger fires only from batch size,
        // so drain them all through one force_flush.
        for _ in 0..12 {
            processor.on_end(new_test_export_span_data());
        }
        let started = Instant::now();
        processor.force_flush().expect("flush should succeed");
        let elapsed = started.elapsed();

        assert_eq!(max_in_flight.load(Ordering::SeqCst), 3);
        assert!(
            elapsed < Duration::from_millis(400),
            "three 150 ms exports must overlap, took {elapsed:?}"
        );
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.exported, 12);
        assert_eq!(snapshot.in_flight, 0);
        assert_eq!(snapshot.queue_depth, 0);
    }

    #[test]
    fn queue_bound_is_exact_and_accounted() {
        #[derive(Debug)]
        struct NeverExporter;
        impl SpanExporter for NeverExporter {
            fn export(&self, _batch: Vec<SpanData>) -> impl Future<Output = OTelSdkResult> + Send {
                async { Ok(()) }
            }
        }
        let config = BatchProcessorConfig {
            max_queue_size: 6,
            max_export_batch_size: 6,
            max_concurrent_exports: 1,
            scheduled_delay: Duration::from_secs(60),
            export_timeout: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(1),
            retry: RetryPolicy::disabled(),
        };
        let metrics = BatchMetrics::default();
        let processor = BoundedBatchSpanProcessor::new(NeverExporter, config, metrics.clone())
            .expect("processor should start");
        // Fill the queue without reaching the export trigger (depth >= batch size
        // notifies the worker, so the 6th span triggers a drain; count only 5).
        for _ in 0..5 {
            processor.on_end(new_test_export_span_data());
        }
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.queued, 5);
        assert_eq!(snapshot.queue_depth, 5);
        assert_eq!(snapshot.dropped_queue_full, 0);
        assert!(processor.span_sender.try_send(Box::new(new_test_export_span_data())).is_ok());
        assert!(
            processor.span_sender.try_send(Box::new(new_test_export_span_data())).is_err(),
            "the channel must hold exactly max_queue_size spans"
        );
    }

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn concurrent_export_config_is_bounded() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        unsafe {
            env::set_var("OTEL_BSP_MAX_CONCURRENT_EXPORTS", "9");
        }
        let error = BatchProcessorConfig::from_env().expect_err("9 must be rejected");
        assert!(error.contains("OTEL_BSP_MAX_CONCURRENT_EXPORTS"));
        unsafe {
            env::set_var("OTEL_BSP_MAX_CONCURRENT_EXPORTS", "4");
        }
        let config = BatchProcessorConfig::from_env().expect("4 must be accepted");
        assert_eq!(config.max_concurrent_exports, 4);
        unsafe {
            env::remove_var("OTEL_BSP_MAX_CONCURRENT_EXPORTS");
        }
        assert_eq!(BatchProcessorConfig::from_env().unwrap().max_concurrent_exports, 1);
    }

    #[derive(Debug)]
    struct SlowShutdownExporter;

    impl SpanExporter for SlowShutdownExporter {
        fn export(&self, _batch: Vec<SpanData>) -> impl Future<Output = OTelSdkResult> + Send {
            async { Ok(()) }
        }

        fn shutdown_with_timeout(&mut self, _timeout: Duration) -> OTelSdkResult {
            thread::sleep(Duration::from_millis(500));
            Ok(())
        }
    }

    #[test]
    fn shutdown_never_waits_for_a_stuck_exporter_past_the_budget() {
        let config = BatchProcessorConfig {
            max_queue_size: 8,
            max_export_batch_size: 4,
            max_concurrent_exports: 1,
            scheduled_delay: Duration::from_secs(60),
            export_timeout: Duration::from_secs(1),
            shutdown_timeout: Duration::from_millis(40),
            retry: RetryPolicy::disabled(),
        };
        let processor =
            BoundedBatchSpanProcessor::new(SlowShutdownExporter, config, BatchMetrics::default())
                .expect("processor should start");

        let started = Instant::now();
        let result = processor.shutdown_with_timeout(Duration::from_secs(5));

        assert!(matches!(result, Err(OTelSdkError::Timeout(_))));
        assert!(started.elapsed() < Duration::from_millis(200));
        assert!(matches!(
            processor.shutdown_with_timeout(Duration::from_secs(5)),
            Err(OTelSdkError::AlreadyShutdown)
        ));
    }

    fn retry_test_config(retry: RetryPolicy, shutdown_timeout: Duration) -> BatchProcessorConfig {
        BatchProcessorConfig {
            max_queue_size: 64,
            max_export_batch_size: 4,
            max_concurrent_exports: 1,
            scheduled_delay: Duration::from_secs(60),
            export_timeout: Duration::from_secs(1),
            shutdown_timeout,
            retry,
        }
    }

    fn fast_retry(max_attempts: usize) -> RetryPolicy {
        RetryPolicy {
            max_attempts,
            max_elapsed: Duration::from_secs(5),
            initial_backoff: Duration::from_millis(1),
        }
    }

    fn grpc_unavailable() -> OTelSdkError {
        OTelSdkError::InternalFailure(
            "code: 'The service is currently unavailable', message: \"tcp connect error\", source: tonic::transport::Error(Transport, ConnectError(ConnectError(\"tcp connect error\", 127.0.0.1:1, Os { code: 111, kind: ConnectionRefused, message: \"Connection refused\" })))".to_string(),
        )
    }

    fn grpc_invalid_argument() -> OTelSdkError {
        OTelSdkError::InternalFailure(
            "code: 'Client specified an invalid argument', message: \"bad resource\"".to_string(),
        )
    }

    /// Fails the first `failures` export calls with `error`, then succeeds.
    /// Each call optionally takes `delay` so elapsed-budget tests are timing
    /// driven rather than attempt driven.
    #[derive(Debug)]
    struct FlakyExporter {
        failures: usize,
        error: fn() -> OTelSdkError,
        delay: Duration,
        calls: Arc<AtomicUsize>,
    }

    impl FlakyExporter {
        fn new(failures: usize, error: fn() -> OTelSdkError) -> Self {
            Self {
                failures,
                error,
                delay: Duration::ZERO,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl SpanExporter for FlakyExporter {
        fn export(&self, _batch: Vec<SpanData>) -> impl Future<Output = OTelSdkResult> + Send {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let result = if call < self.failures {
                Err((self.error)())
            } else {
                Ok(())
            };
            let delay = self.delay;
            async move {
                if !delay.is_zero() {
                    Delay {
                        deadline: Instant::now() + delay,
                        armed: false,
                    }
                    .await;
                }
                result
            }
        }
    }

    fn assert_drain_invariant(snapshot: &BatchMetricsSnapshot) {
        assert_eq!(
            snapshot.sampled_ended,
            snapshot.exported
                + snapshot.dropped_queue_full
                + snapshot.dropped_export_failure
                + snapshot.dropped_shutdown,
            "drain invariant violated: {snapshot:?}"
        );
        assert_eq!(snapshot.queue_depth, 0);
        assert_eq!(snapshot.in_flight, 0);
    }

    #[test]
    fn retryable_failures_are_retried_until_the_batch_exports() {
        let exporter = FlakyExporter::new(2, grpc_unavailable);
        let calls = exporter.calls.clone();
        let metrics = BatchMetrics::default();
        let processor = BoundedBatchSpanProcessor::new(
            exporter,
            retry_test_config(fast_retry(3), Duration::from_secs(2)),
            metrics.clone(),
        )
        .expect("processor should start");

        for _ in 0..4 {
            processor.on_end(new_test_export_span_data());
        }
        processor
            .force_flush()
            .expect("flush should succeed after retries");

        let snapshot = metrics.snapshot();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(snapshot.exported, 4);
        assert_eq!(snapshot.export_retries, 2);
        assert_eq!(snapshot.export_retry_recovered, 1);
        assert_eq!(snapshot.export_failures, 0);
        assert_eq!(snapshot.dropped_export_failure, 0);
        assert_drain_invariant(&snapshot);
    }

    #[test]
    fn terminal_errors_fail_the_batch_after_one_attempt() {
        let exporter = FlakyExporter::new(usize::MAX, grpc_invalid_argument);
        let calls = exporter.calls.clone();
        let metrics = BatchMetrics::default();
        let processor = BoundedBatchSpanProcessor::new(
            exporter,
            retry_test_config(fast_retry(5), Duration::from_secs(2)),
            metrics.clone(),
        )
        .expect("processor should start");

        for _ in 0..4 {
            processor.on_end(new_test_export_span_data());
        }
        // The size trigger may already have run the round; the flush result is
        // Ok or the terminal error depending on timing. The metrics are the contract.
        let _ = processor.force_flush();

        let snapshot = metrics.snapshot();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(snapshot.export_retries, 0);
        assert_eq!(snapshot.export_retry_recovered, 0);
        assert_eq!(snapshot.export_failures, 1);
        assert_eq!(snapshot.dropped_export_failure, 4);
        assert_eq!(snapshot.exported, 0);
        assert_drain_invariant(&snapshot);
    }

    #[test]
    fn retry_stops_when_the_attempt_budget_is_exhausted() {
        let exporter = FlakyExporter::new(usize::MAX, grpc_unavailable);
        let calls = exporter.calls.clone();
        let metrics = BatchMetrics::default();
        let processor = BoundedBatchSpanProcessor::new(
            exporter,
            retry_test_config(fast_retry(3), Duration::from_secs(2)),
            metrics.clone(),
        )
        .expect("processor should start");

        for _ in 0..4 {
            processor.on_end(new_test_export_span_data());
        }
        // The size trigger may already have run the round; the flush result is
        // Ok or the terminal error depending on timing. The metrics are the contract.
        let _ = processor.force_flush();

        let snapshot = metrics.snapshot();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(snapshot.export_retries, 2);
        assert_eq!(snapshot.export_retry_recovered, 0);
        assert_eq!(snapshot.export_failures, 1);
        assert_eq!(snapshot.dropped_export_failure, 4);
        assert_eq!(snapshot.exported, 0);
        assert_drain_invariant(&snapshot);
    }

    #[test]
    fn retry_stops_when_the_elapsed_budget_is_exhausted() {
        let mut exporter = FlakyExporter::new(usize::MAX, grpc_unavailable);
        exporter.delay = Duration::from_millis(40);
        let calls = exporter.calls.clone();
        let metrics = BatchMetrics::default();
        let policy = RetryPolicy {
            max_attempts: 10,
            max_elapsed: Duration::from_millis(100),
            initial_backoff: Duration::from_millis(1),
        };
        let processor = BoundedBatchSpanProcessor::new(
            exporter,
            retry_test_config(policy, Duration::from_secs(2)),
            metrics.clone(),
        )
        .expect("processor should start");

        for _ in 0..4 {
            processor.on_end(new_test_export_span_data());
        }
        let started = Instant::now();
        // The size trigger may already have run the round; the flush result is
        // Ok or the terminal error depending on timing. The metrics are the contract.
        let _ = processor.force_flush();
        let elapsed = started.elapsed();

        let snapshot = metrics.snapshot();
        let calls = calls.load(Ordering::SeqCst);
        assert!(
            (2..=3).contains(&calls),
            "40 ms attempts inside a 100 ms budget must stop after 2-3 attempts, made {calls}"
        );
        assert_eq!(snapshot.export_retries, calls - 1);
        assert!(
            elapsed < Duration::from_millis(400),
            "the elapsed budget must cut the round short, took {elapsed:?}"
        );
        assert_eq!(snapshot.export_failures, 1);
        assert_eq!(snapshot.dropped_export_failure, 4);
        assert_eq!(snapshot.exported, 0);
        assert_drain_invariant(&snapshot);
    }

    #[test]
    fn a_single_attempt_disables_retry() {
        let exporter = FlakyExporter::new(1, grpc_unavailable);
        let calls = exporter.calls.clone();
        let metrics = BatchMetrics::default();
        let processor = BoundedBatchSpanProcessor::new(
            exporter,
            retry_test_config(fast_retry(1), Duration::from_secs(2)),
            metrics.clone(),
        )
        .expect("processor should start");

        for _ in 0..4 {
            processor.on_end(new_test_export_span_data());
        }
        // The size trigger may already have run the round; the flush result is
        // Ok or the terminal error depending on timing. The metrics are the contract.
        let _ = processor.force_flush();

        let snapshot = metrics.snapshot();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(snapshot.export_retries, 0);
        assert_eq!(snapshot.export_failures, 1);
        assert_eq!(snapshot.dropped_export_failure, 4);
        assert_drain_invariant(&snapshot);
    }

    #[test]
    fn shutdown_during_backoff_completes_within_the_budget() {
        let exporter = FlakyExporter::new(usize::MAX, grpc_unavailable);
        let calls = exporter.calls.clone();
        let metrics = BatchMetrics::default();
        let policy = RetryPolicy {
            max_attempts: 4,
            max_elapsed: Duration::from_secs(30),
            initial_backoff: Duration::from_millis(5_000),
        };
        let processor = BoundedBatchSpanProcessor::new(
            exporter,
            retry_test_config(policy, Duration::from_millis(100)),
            metrics.clone(),
        )
        .expect("processor should start");

        // Batch-size trigger starts the round; the first attempt fails at once
        // and the worker enters a 5 s backoff.
        for _ in 0..4 {
            processor.on_end(new_test_export_span_data());
        }
        thread::sleep(Duration::from_millis(50));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let started = Instant::now();
        let result = processor.shutdown_with_timeout(Duration::from_secs(5));
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_millis(120),
            "shutdown must not wait for the backoff, took {elapsed:?}"
        );
        assert!(result.is_ok(), "shutdown must complete cleanly: {result:?}");
        let snapshot = metrics.snapshot();
        // The shutdown request cuts the backoff short, the remaining attempts
        // run immediately and the batch ends as an export failure.
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert_eq!(snapshot.export_retries, 3);
        assert_eq!(snapshot.export_failures, 1);
        assert_eq!(snapshot.dropped_export_failure, 4);
        assert_eq!(snapshot.dropped_shutdown, 0);
        assert_drain_invariant(&snapshot);
    }

    #[test]
    fn retry_config_is_bounded() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let cases: [(&str, &str, bool); 11] = [
            ("OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS", "0", false),
            ("OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS", "11", false),
            ("OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS", "abc", false),
            ("OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS", "10", true),
            ("OTEL_PHP_EXPORT_RETRY_MAX_ATTEMPTS", "1", true),
            ("OTEL_PHP_EXPORT_RETRY_MAX_ELAPSED", "30001", false),
            ("OTEL_PHP_EXPORT_RETRY_MAX_ELAPSED", "-1", false),
            ("OTEL_PHP_EXPORT_RETRY_MAX_ELAPSED", "0", true),
            ("OTEL_PHP_EXPORT_RETRY_INITIAL_BACKOFF", "0", false),
            ("OTEL_PHP_EXPORT_RETRY_INITIAL_BACKOFF", "5001", false),
            ("OTEL_PHP_EXPORT_RETRY_INITIAL_BACKOFF", "5000", true),
        ];
        for (name, value, accepted) in cases {
            unsafe {
                env::set_var(name, value);
            }
            let result = RetryPolicy::from_env();
            unsafe {
                env::remove_var(name);
            }
            match result {
                Ok(_) if accepted => {}
                Err(error) if !accepted => assert!(error.contains(name), "{error}"),
                other => panic!("{name}={value}: unexpected {other:?}"),
            }
        }
        let defaults = RetryPolicy::from_env().expect("defaults must parse");
        assert_eq!(
            defaults,
            RetryPolicy {
                max_attempts: 3,
                max_elapsed: Duration::from_millis(5_000),
                initial_backoff: Duration::from_millis(100),
            }
        );
    }

    #[test]
    fn export_errors_are_classified_by_otlp_retry_guidance() {
        use ExportErrorKind::{Retryable, Terminal};
        let internal = |message: &str| OTelSdkError::InternalFailure(message.to_string());
        let cases = [
            // tonic 0.14 Status Display: code: '<description>', message: "...", source: ...
            (grpc_unavailable(), Retryable),
            (internal("code: 'The operation was cancelled', message: \"Timeout expired\", source: tonic::transport::Error(Transport, TimeoutExpired(()))"), Retryable),
            (internal("code: 'Deadline expired before operation could complete', message: \"timed out\""), Retryable),
            (internal("code: 'The operation was aborted', message: \"\""), Retryable),
            (internal("code: 'Operation was attempted past the valid range', message: \"\""), Retryable),
            (internal("code: 'Unrecoverable data loss or corruption', message: \"\""), Retryable),
            (internal("code: 'Some resource has been exhausted', message: \"too many requests\""), Retryable),
            (grpc_invalid_argument(), Terminal),
            (internal("code: 'The request does not have valid authentication credentials', message: \"provided authorization does not match expected scheme or token\""), Terminal),
            (internal("code: 'The caller does not have permission to execute the specified operation', message: \"\""), Terminal),
            (internal("code: 'Operation is not implemented or not supported', message: \"unknown service\""), Terminal),
            (internal("code: 'Internal error', message: \"h2 protocol error: connection reset\""), Terminal),
            (internal("code: 'Some requested entity was not found', message: \"\""), Terminal),
            (internal("code: 'Some entity that we attempted to create already exists', message: \"\""), Terminal),
            // gRPC UNKNOWN carrying a transport failure is transient.
            (internal("code: 'Unknown error', message: \"transport error\", source: Some(tonic::transport::Error(Transport, hyper_util::client::legacy::Error(SendRequest, hyper::Error(Io, Os { code: 104, kind: ConnectionReset, message: \"Connection reset by peer\" }))))"), Retryable),
            (internal("code: 'Unknown error', message: \"protocol error: missing grpc-status trailer\""), Terminal),
            // reqwest blocking client Debug shapes (http/protobuf).
            (internal("reqwest::Error { kind: Request, url: \"http://127.0.0.1:1/v1/traces\", source: hyper_util::client::legacy::Error(Connect, ConnectError(\"tcp connect error\", 127.0.0.1:1, Os { code: 111, kind: ConnectionRefused, message: \"Connection refused\" })) }"), Retryable),
            (internal("reqwest::Error { kind: Request, url: \"http://collector:4318/v1/traces\", source: hyper_util::client::legacy::Error(Connect, ConnectError(\"dns error\", Custom { kind: Uncategorized, error: \"failed to lookup address information: Name or service not known\" })) }"), Retryable),
            (internal("reqwest::Error { kind: Request, url: \"http://blackhole:4318/v1/traces\", source: TimedOut }"), Retryable),
            (internal("reqwest::Error { kind: Status(429, Some(\"Too Many Requests\")), url: \"http://collector:4318/v1/traces\" }"), Retryable),
            (internal("reqwest::Error { kind: Status(502, Some(\"Bad Gateway\")), url: \"http://collector:4318/v1/traces\" }"), Retryable),
            (internal("reqwest::Error { kind: Status(503, Some(\"Service Unavailable\")), url: \"http://collector:4318/v1/traces\" }"), Retryable),
            (internal("reqwest::Error { kind: Status(504, Some(\"Gateway Timeout\")), url: \"http://collector:4318/v1/traces\" }"), Retryable),
            (internal("reqwest::Error { kind: Status(400, Some(\"Bad Request\")), url: \"http://collector:4318/v1/traces\" }"), Terminal),
            (internal("reqwest::Error { kind: Status(401, None), url: \"http://collector-auth:4318/v1/traces\" }"), Terminal),
            (internal("reqwest::Error { kind: Status(403, Some(\"Forbidden\")), url: \"http://collector:4318/v1/traces\" }"), Terminal),
            (internal("reqwest::Error { kind: Status(404, None), url: \"http://collector-auth:4318/nonexistent/v1/traces\" }"), Terminal),
            (internal("reqwest::Error { kind: Status(413, Some(\"Payload Too Large\")), url: \"http://collector:4318/v1/traces\" }"), Terminal),
            (internal("reqwest::Error { kind: Status(500, Some(\"Internal Server Error\")), url: \"http://collector:4318/v1/traces\" }"), Terminal),
            (internal("reqwest::Error { kind: Status(501, Some(\"Not Implemented\")), url: \"http://collector:4318/v1/traces\" }"), Terminal),
            (internal("reqwest::Error { kind: Builder, source: RelativeUrlWithoutBase }"), Terminal),
            // opentelemetry-otlp HTTP exporter status line (non-reqwest clients).
            (internal("OpenTelemetry trace export failed. Url: http://collector:4318/v1/traces, Status Code: 503, Response: b\"\""), Retryable),
            (internal("OpenTelemetry trace export failed. Url: http://collector:4318/v1/traces, Status Code: 429, Response: b\"slow down\""), Retryable),
            (internal("OpenTelemetry trace export failed. Url: http://collector:4318/v1/traces, Status Code: 400, Response: b\"bad\""), Terminal),
            (internal("OpenTelemetry trace export failed. Url: http://collector:4318/v1/traces, Status Code: 500, Response: b\"\""), Terminal),
            // hyper client transport failures.
            (internal("client error (Connect): error trying to connect: tcp connect error: Connection refused (os error 111)"), Retryable),
            (internal("connection closed before message completed"), Retryable),
            (internal("operation timed out"), Retryable),
            (internal("Mutex lock failed: poisoned"), Terminal),
        ];
        for (error, expected) in cases {
            assert_eq!(classify_export_error(&error), expected, "{error}");
        }
        assert_eq!(
            classify_export_error(&OTelSdkError::Timeout(Duration::from_secs(1))),
            Retryable
        );
        assert_eq!(
            classify_export_error(&OTelSdkError::AlreadyShutdown),
            Terminal
        );
    }
}
