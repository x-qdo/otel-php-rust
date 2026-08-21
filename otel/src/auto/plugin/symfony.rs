use crate::{
    auto::{
        execute_data::{get_exec_data_flag, remove_exec_data_flag, set_exec_data_flag},
        plugin::{Handler, HandlerCallbacks, HandlerList, HandlerSlice, Plugin},
        utils::{record_exception, start_and_activate_span},
    },
    config::trace_attributes,
    context::storage::take_guard,
    request::get_request_details,
    trace::{local_root_span::get_local_root_span_context, tracer_provider},
};
use opentelemetry::{
    Context, KeyValue,
    trace::{SpanKind, Status, TraceContextExt, TracerProvider},
};
use opentelemetry_semantic_conventions as SemConv;
use phper::{
    classes::ClassEntry,
    objects::ZObj,
    values::{ExecuteData, ZVal},
};
use std::sync::Arc;

/// Symfony HttpKernel instrumentation.
///
/// Routing has completed by the `handle` post-hook, so the native request span
/// can be renamed using Symfony's low-cardinality route name.
pub struct SymfonyPlugin {
    handlers: HandlerList,
}

impl Default for SymfonyPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl SymfonyPlugin {
    pub fn new() -> Self {
        Self {
            handlers: vec![
                Arc::new(SymfonyHttpKernelHandler),
                Arc::new(SymfonyConsoleCommandHandler),
            ],
        }
    }
}

struct SymfonyConsoleCommandHandler;

impl Handler for SymfonyConsoleCommandHandler {
    fn get_targets(&self) -> Vec<(Option<&'static str>, &'static str)> {
        vec![(Some(r"Symfony\Component\Console\Command\Command"), "run")]
    }

    fn get_callbacks(&self) -> HandlerCallbacks {
        HandlerCallbacks {
            pre_observe: Some(Box::new(|exec_data| unsafe {
                Self::pre_callback(exec_data)
            })),
            post_observe: Some(Box::new(|exec_data, retval, exception| unsafe {
                Self::post_callback(exec_data, retval, exception)
            })),
        }
    }
}

impl SymfonyConsoleCommandHandler {
    unsafe fn pre_callback(exec_data: *mut ExecuteData) {
        // Laravel commands inherit Symfony Command::run and have their own
        // more-specific execute hook. Avoid producing two spans for one command.
        if is_laravel_command(exec_data) {
            set_exec_data_flag(exec_data, false);
            return;
        }
        set_exec_data_flag(exec_data, true);

        let command = unsafe { exec_data.as_mut() }
            .and_then(ExecuteData::get_this_mut)
            .and_then(|object| call_string(object, "getName"))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "unknown".to_string());
        let tracer = tracer_provider::get_tracer_provider().tracer("php.otel.auto.symfony");
        start_and_activate_span(
            tracer,
            &format!("Command {command}"),
            vec![
                KeyValue::new(trace_attributes::PHP_FRAMEWORK_NAME, "symfony"),
                KeyValue::new("console.command", command),
            ],
            exec_data,
            SpanKind::Internal,
        );
    }

    unsafe fn post_callback(
        exec_data: *mut ExecuteData,
        retval: &mut ZVal,
        exception: Option<&mut ZObj>,
    ) {
        let traced = get_exec_data_flag(exec_data).unwrap_or(false);
        remove_exec_data_flag(exec_data);
        if !traced {
            return;
        }

        let _guard = take_guard(exec_data);
        let context = Context::current();
        if let Some(exception) = exception {
            record_exception(&context, exception);
        } else if retval.as_long().is_some_and(|exit_code| exit_code != 0) {
            context.span().set_status(Status::error(""));
        }
    }
}

fn is_laravel_command(exec_data: *mut ExecuteData) -> bool {
    let Some(exec_data) = (unsafe { exec_data.as_mut() }) else {
        return false;
    };
    let Some(command) = exec_data.get_this_mut() else {
        return false;
    };
    let Ok(laravel_command) = ClassEntry::from_globals(r"Illuminate\Console\Command") else {
        return false;
    };

    command.get_class().is_instance_of(laravel_command)
}

impl Plugin for SymfonyPlugin {
    fn get_handlers(&self) -> &HandlerSlice {
        &self.handlers
    }

    fn get_name(&self) -> &str {
        "symfony"
    }
}

struct SymfonyHttpKernelHandler;

impl Handler for SymfonyHttpKernelHandler {
    fn get_targets(&self) -> Vec<(Option<&'static str>, &'static str)> {
        vec![(Some(r"Symfony\Component\HttpKernel\HttpKernel"), "handle")]
    }

    fn get_callbacks(&self) -> HandlerCallbacks {
        HandlerCallbacks {
            pre_observe: None,
            post_observe: Some(Box::new(|exec_data, _retval, _exception| unsafe {
                Self::post_callback(exec_data)
            })),
        }
    }
}

impl SymfonyHttpKernelHandler {
    unsafe fn post_callback(exec_data: *mut ExecuteData) {
        let Some(context) = get_local_root_span_context() else {
            tracing::debug!("Auto::Symfony::post - no local root span found, skipping");
            return;
        };
        let Some(exec_data) = (unsafe { exec_data.as_mut() }) else {
            return;
        };
        // HttpKernelInterface::MAIN_REQUEST is 1. Sub-requests must not rename
        // the request-level SERVER span with their controller or route.
        if exec_data.num_args() > 1 && exec_data.get_parameter(1).as_long() != Some(1) {
            return;
        }
        context.span().set_attribute(KeyValue::new(
            trace_attributes::PHP_FRAMEWORK_NAME,
            "symfony",
        ));
        let Some(request) = exec_data.get_mut_parameter(0).as_mut_z_obj() else {
            tracing::debug!("Auto::Symfony::post - request argument is not an object");
            return;
        };

        let method = call_string(request, "getMethod")
            .or_else(|| get_request_details().method)
            .unwrap_or_else(|| "unknown".to_string());
        let Some(attributes) = request.get_mut_property("attributes").as_mut_z_obj() else {
            return;
        };

        if let Some(route_name) =
            parameter_bag_string(attributes, "_route").filter(|route| !route.is_empty())
        {
            let span = context.span();
            span.update_name(format!("{method} {route_name}"));
            span.set_attribute(KeyValue::new(SemConv::trace::HTTP_ROUTE, route_name));
        }

        if let Some(controller_name) = parameter_bag_string(attributes, "_controller")
            && let Some((controller, action)) = split_controller_name(&controller_name)
        {
            let span = context.span();
            span.set_attribute(KeyValue::new(
                trace_attributes::PHP_FRAMEWORK_CONTROLLER_NAME,
                controller,
            ));
            span.set_attribute(KeyValue::new(
                trace_attributes::PHP_FRAMEWORK_ACTION_NAME,
                action,
            ));
        }
    }
}

fn call_string(object: &mut ZObj, method: &str) -> Option<String> {
    object.call(method, []).ok().and_then(|value| {
        value
            .as_z_str()
            .and_then(|string| string.to_str().ok().map(str::to_owned))
    })
}

fn parameter_bag_string(attributes: &mut ZObj, key: &str) -> Option<String> {
    attributes
        .call("get", &mut [ZVal::from(key)])
        .ok()
        .and_then(|value| {
            value
                .as_z_str()
                .and_then(|string| string.to_str().ok().map(str::to_owned))
        })
}

fn split_controller_name(controller_name: &str) -> Option<(String, String)> {
    let (controller, action) = controller_name
        .rsplit_once("::")
        .or_else(|| controller_name.rsplit_once(':'))?;
    if controller.is_empty() || action.is_empty() {
        return None;
    }

    Some((controller.to_string(), action.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_actions_are_split() {
        assert_eq!(
            split_controller_name(r"App\Controller\UserController::show"),
            Some((
                r"App\Controller\UserController".to_string(),
                "show".to_string(),
            ))
        );
        assert_eq!(
            split_controller_name(r"App\Controller\UserController:show"),
            Some((
                r"App\Controller\UserController".to_string(),
                "show".to_string(),
            ))
        );
        assert_eq!(split_controller_name("service_controller"), None);
    }
}
