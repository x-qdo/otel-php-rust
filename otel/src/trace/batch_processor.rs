use futures_util::future::join_all;
use opentelemetry::Context;
use opentelemetry_sdk::{
    Resource,
    error::{OTelSdkError, OTelSdkResult},
    trace::{Span, SpanData, SpanExporter, SpanProcessor},
};
use std::{
    env, fmt,
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

        Ok(Self {
            max_queue_size,
            max_export_batch_size,
            max_concurrent_exports,
            scheduled_delay: Duration::from_millis(scheduled_delay_ms),
            export_timeout: Duration::from_millis(export_timeout_ms),
            shutdown_timeout: Duration::from_millis(shutdown_timeout_ms),
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
    match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0 && *value <= maximum)
            .ok_or_else(|| format!("{name} must be an integer between 1 and {maximum}")),
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
    abort_export: Arc<AtomicBool>,
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
        let abort_export = Arc::new(AtomicBool::new(false));

        let worker_config = config.clone();
        let worker_metrics = metrics.clone();
        let worker_export_pending = export_pending.clone();
        let worker_abort_export = abort_export.clone();
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
                    worker_abort_export,
                )
            })
            .map_err(|error| format!("failed to create trace exporter worker: {error}"))?;

        Ok(Self {
            span_sender,
            control_sender,
            handle: Mutex::new(Some(handle)),
            metrics,
            export_pending,
            abort_export,
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
        let (sender, receiver) = mpsc::sync_channel(1);
        let result = self.wait_for_control(
            ControlMessage::Shutdown(sender),
            receiver,
            effective_timeout,
        );

        if matches!(result, Err(OTelSdkError::Timeout(_))) {
            self.abort_export.store(true, Ordering::SeqCst);
            let remaining = self.metrics.inner.queue_depth.load(Ordering::Relaxed)
                + self.metrics.inner.in_flight.load(Ordering::Relaxed);
            self.metrics
                .inner
                .dropped_shutdown
                .fetch_add(remaining, Ordering::Relaxed);
        } else if result.is_ok() {
            let handle = self.handle.lock().ok().and_then(|mut handle| handle.take());
            if let Some(handle) = handle {
                if handle.join().is_err() {
                    return Err(OTelSdkError::InternalFailure(
                        "trace exporter worker panicked".to_string(),
                    ));
                }
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
    abort_export: Arc<AtomicBool>,
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
                let _ =
                    export_available(&exporter, &span_receiver, &config, &metrics, &abort_export);
                last_export = Instant::now();
            }
            Ok(ControlMessage::ForceFlush(sender)) => {
                tracing::debug!("BoundedBatchProcessor.ExportingDueToForceFlush");
                let result =
                    export_available(&exporter, &span_receiver, &config, &metrics, &abort_export)
                        .and_then(|_| exporter.force_flush());
                let _ = sender.send(result);
                last_export = Instant::now();
            }
            Ok(ControlMessage::Shutdown(sender)) => {
                tracing::debug!("BoundedBatchProcessor.ExportingDueToShutdown");
                let result =
                    export_available(&exporter, &span_receiver, &config, &metrics, &abort_export)
                        .and_then(|_| exporter.shutdown_with_timeout(config.export_timeout));
                if abort_export.load(Ordering::SeqCst) {
                    discard_remaining(&span_receiver, &metrics);
                }
                let _ = sender.send(result);
                tracing::debug!("BoundedBatchProcessor.ThreadStopped");
                break;
            }
            Ok(ControlMessage::SetResource(resource)) => exporter.set_resource(&resource),
            Err(RecvTimeoutError::Timeout) => {
                let _ =
                    export_available(&exporter, &span_receiver, &config, &metrics, &abort_export);
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
    abort_export: &AtomicBool,
) -> OTelSdkResult
where
    E: SpanExporter + 'static,
{
    let target = metrics.inner.queue_depth.load(Ordering::Relaxed);
    let mut processed = 0;
    let mut first_error = None;

    while processed < target && !abort_export.load(Ordering::SeqCst) {
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

        let sizes: Vec<usize> = batches.iter().map(Vec::len).collect();
        let in_flight: usize = sizes.iter().sum();
        processed += in_flight;
        metrics.inner.in_flight.store(in_flight, Ordering::Relaxed);
        let results = futures_executor::block_on(join_all(
            batches.into_iter().map(|batch| exporter.export(batch)),
        ));
        metrics.inner.in_flight.store(0, Ordering::Relaxed);

        if abort_export.load(Ordering::SeqCst) {
            continue;
        }
        for (result, batch_size) in results.into_iter().zip(sizes) {
            match result {
                Ok(()) => {
                    metrics
                        .inner
                        .exported
                        .fetch_add(batch_size, Ordering::Relaxed);
                }
                Err(error) => {
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
                        first_error = Some(error.to_string());
                    }
                }
            }
        }
    }

    if let Some(error) = first_error {
        Err(OTelSdkError::InternalFailure(error))
    } else {
        Ok(())
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

    #[test]
    fn concurrent_export_config_is_bounded() {
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
}
