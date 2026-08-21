use crate::{
    auto::{
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
    objects::ZObj,
    values::{ExecuteData, ZVal},
};
use std::sync::Arc;

/// Laravel HTTP instrumentation.
///
/// The extension already owns the request SERVER span, so this plugin enriches
/// that span after Laravel has resolved the route instead of creating a second
/// server span around the HTTP kernel.
pub struct LaravelPlugin {
    handlers: HandlerList,
}

impl Default for LaravelPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl LaravelPlugin {
    pub fn new() -> Self {
        Self {
            handlers: vec![
                Arc::new(LaravelHttpKernelHandler),
                Arc::new(LaravelConsoleCommandHandler),
            ],
        }
    }
}

struct LaravelConsoleCommandHandler;

impl Handler for LaravelConsoleCommandHandler {
    fn get_targets(&self) -> Vec<(Option<&'static str>, &'static str)> {
        vec![(Some(r"Illuminate\Console\Command"), "execute")]
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

impl LaravelConsoleCommandHandler {
    unsafe fn pre_callback(exec_data: *mut ExecuteData) {
        let command = unsafe { exec_data.as_mut() }
            .and_then(ExecuteData::get_this_mut)
            .and_then(|object| call_string(object, "getName"))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "unknown".to_string());
        let tracer = tracer_provider::get_tracer_provider().tracer("php.otel.auto.laravel");
        start_and_activate_span(
            tracer,
            &format!("Command {command}"),
            vec![
                KeyValue::new(trace_attributes::PHP_FRAMEWORK_NAME, "laravel"),
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
        let _guard = take_guard(exec_data);
        let context = Context::current();
        if let Some(exception) = exception {
            record_exception(&context, exception);
        } else if retval.as_long().is_some_and(|exit_code| exit_code != 0) {
            context.span().set_status(Status::error(""));
        }
    }
}

impl Plugin for LaravelPlugin {
    fn get_handlers(&self) -> &HandlerSlice {
        &self.handlers
    }

    fn get_name(&self) -> &str {
        "laravel"
    }
}

struct LaravelHttpKernelHandler;

impl Handler for LaravelHttpKernelHandler {
    fn get_targets(&self) -> Vec<(Option<&'static str>, &'static str)> {
        vec![(Some(r"Illuminate\Contracts\Http\Kernel"), "handle")]
    }

    fn get_callbacks(&self) -> HandlerCallbacks {
        HandlerCallbacks {
            pre_observe: Some(Box::new(|_exec_data| {
                set_framework_name();
            })),
            post_observe: Some(Box::new(|exec_data, _retval, _exception| unsafe {
                Self::post_callback(exec_data)
            })),
        }
    }
}

impl LaravelHttpKernelHandler {
    unsafe fn post_callback(exec_data: *mut ExecuteData) {
        let Some(context) = get_local_root_span_context() else {
            tracing::debug!("Auto::Laravel::post - no local root span found, skipping");
            return;
        };
        let Some(exec_data) = (unsafe { exec_data.as_mut() }) else {
            return;
        };
        let Some(request) = exec_data.get_mut_parameter(0).as_mut_z_obj() else {
            tracing::debug!("Auto::Laravel::post - request argument is not an object");
            return;
        };

        let method = call_string(request, "method")
            .or_else(|| get_request_details().method)
            .unwrap_or_else(|| "unknown".to_string());
        let Some(mut route_value) = request.call("route", []).ok() else {
            return;
        };
        let Some(route) = route_value.as_mut_z_obj() else {
            return;
        };
        let Some(route_uri) = call_string(route, "uri").filter(|route| !route.is_empty()) else {
            return;
        };

        let normalized_route = normalize_route(&route_uri);
        let span = context.span();
        span.update_name(format!("{method} {normalized_route}"));
        span.set_attribute(KeyValue::new(SemConv::trace::HTTP_ROUTE, normalized_route));

        if let Some(action_name) = call_string(route, "getActionName")
            && let Some((controller, action)) = split_action_name(&action_name)
        {
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

fn set_framework_name() {
    if let Some(context) = get_local_root_span_context() {
        context.span().set_attribute(KeyValue::new(
            trace_attributes::PHP_FRAMEWORK_NAME,
            "laravel",
        ));
    }
}

fn call_string(object: &mut ZObj, method: &str) -> Option<String> {
    object.call(method, []).ok().and_then(|value| {
        value
            .as_z_str()
            .and_then(|string| string.to_str().ok().map(str::to_owned))
    })
}

fn normalize_route(route: &str) -> String {
    if route.starts_with('/') {
        route.to_string()
    } else {
        format!("/{route}")
    }
}

fn split_action_name(action_name: &str) -> Option<(String, String)> {
    if action_name.eq_ignore_ascii_case("closure") {
        return None;
    }

    let (controller, action) = action_name
        .rsplit_once("::")
        .or_else(|| action_name.rsplit_once('@'))?;
    if controller.is_empty() || action.is_empty() {
        return None;
    }

    Some((controller.to_string(), action.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_is_normalized_with_a_leading_slash() {
        assert_eq!(normalize_route("users/{user}"), "/users/{user}");
        assert_eq!(normalize_route("/"), "/");
    }

    #[test]
    fn controller_actions_are_split() {
        assert_eq!(
            split_action_name(r"App\Http\Controllers\UserController@show"),
            Some((
                r"App\Http\Controllers\UserController".to_string(),
                "show".to_string(),
            ))
        );
        assert_eq!(
            split_action_name(r"App\Http\Controllers\UserController::__invoke"),
            Some((
                r"App\Http\Controllers\UserController".to_string(),
                "__invoke".to_string(),
            ))
        );
        assert_eq!(split_action_name("Closure"), None);
    }
}
