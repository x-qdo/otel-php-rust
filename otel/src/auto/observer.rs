// Handle auto-instrumentation via the observer API (PHP 8+)
use phper::{
    sys,
    values::{
        ExecuteData,
        ZVal,
    }
};
use crate::{
    auto::{
        execute_data::{
            get_fqn,
            get_global_exception,
        },
        plugin::{
            FunctionObserver,
        },
        plugin_manager::{
            get_global as get_plugin_manager,
        },
    },
    panic,
};
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock, RwLock},
};

static FUNCTION_OBSERVERS: OnceLock<RwLock<HashMap<String, Arc<FunctionObserver>>>> = OnceLock::new();

pub fn init() {
    tracing::debug!("Observer::init");
    FUNCTION_OBSERVERS.get_or_init(|| RwLock::new(HashMap::new()));
    unsafe {
        sys::zend_observer_fcall_register(Some(observer_instrument));
    }
    tracing::debug!("registered fcall handlers");
}

// The three observer entry points are `extern "C"` and called by the engine
// for every observed function call; a panic in hook code is contained with
// `panic::contain` so the observed PHP function still runs.

pub unsafe extern "C" fn observer_instrument(execute_data: *mut sys::zend_execute_data) -> sys::zend_observer_fcall_handlers {
    let no_handlers = sys::zend_observer_fcall_handlers {
        begin: None,
        end: None,
    };
    panic::contain(|| unsafe { instrument(execute_data) }).flatten().unwrap_or(no_handlers)
}

unsafe fn instrument(execute_data: *mut sys::zend_execute_data) -> Option<sys::zend_observer_fcall_handlers> {
    let exec_data = unsafe { ExecuteData::try_from_mut_ptr(execute_data) }?;
    let fqn = get_fqn(exec_data)?;
    tracing::trace!("observer::observer_instrument checking: {}", &fqn);
    let plugin_manager = get_plugin_manager()?
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let observer = plugin_manager.get_function_observer(exec_data)?;
    let observers = FUNCTION_OBSERVERS.get()?;
    observers
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(fqn, observer);

    Some(sys::zend_observer_fcall_handlers {
        begin: Some(pre_observe_c_function),
        end: Some(post_observe_c_function),
    })
}

fn observer_for(fqn: &str) -> Option<Arc<FunctionObserver>> {
    FUNCTION_OBSERVERS
        .get()?
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(fqn)
        .cloned()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pre_observe_c_function(execute_data: *mut sys::zend_execute_data) {
    panic::contain(|| unsafe { pre_observe(execute_data) });
}

unsafe fn pre_observe(execute_data: *mut sys::zend_execute_data) {
    let Some(exec_data) = (unsafe { ExecuteData::try_from_mut_ptr(execute_data) }) else {
        return;
    };
    let Some(fqn) = get_fqn(exec_data) else {
        return;
    };
    let Some(observer) = observer_for(&fqn) else {
        return;
    };
    for hook in observer.pre_hooks() {
        tracing::trace!("running pre hook: {}", fqn);
        hook(&mut *exec_data);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn post_observe_c_function(execute_data: *mut sys::zend_execute_data, retval: *mut sys::zval) {
    panic::contain(|| unsafe { post_observe(execute_data, retval) });
}

unsafe fn post_observe(execute_data: *mut sys::zend_execute_data, retval: *mut sys::zval) {
    let Some(exec_data) = (unsafe { ExecuteData::try_from_mut_ptr(execute_data) }) else {
        return;
    };
    let Some(fqn) = get_fqn(exec_data) else {
        return;
    };
    let Some(observer) = observer_for(&fqn) else {
        return;
    };
    let mut null_retval = ZVal::from(());
    let retval = unsafe { ZVal::try_from_mut_ptr(retval) }.unwrap_or(&mut null_retval);
    for hook in observer.post_hooks() {
        tracing::trace!("running post hook: {}", fqn);
        hook(&mut *exec_data, retval, get_global_exception());
    }
}
