use std::{cell::RefCell, collections::HashMap, process};
use tokio::runtime::{Builder, Runtime};

thread_local! {
    // PHP invokes extension APIs on its request thread. Keeping PID-scoped
    // runtime references in thread-local storage avoids inheriting a possibly
    // locked process-global mutex across fork. Old-PID runtimes are leaked in
    // the child intentionally: their threads no longer exist and running their
    // destructor after fork is unsafe.
    static TOKIO_RUNTIMES: RefCell<HashMap<u32, &'static Runtime>> = RefCell::new(HashMap::new());
}

pub fn init_tokio_runtime() -> Result<&'static Runtime, std::io::Error> {
    let pid = process::id();
    TOKIO_RUNTIMES.with(|runtimes| {
        if let Some(runtime) = runtimes.borrow().get(&pid).copied() {
            return Ok(runtime);
        }

        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(2)
            .thread_name("OpenTelemetry.ExportRuntime")
            .enable_all()
            .build()?;
        let runtime = Box::leak(Box::new(runtime));
        runtimes.borrow_mut().insert(pid, runtime);
        Ok(runtime)
    })
}
