use std::{
    cell::Cell,
    env,
    sync::atomic::{AtomicBool, Ordering},
};

const CAPTURE_SENSITIVE_DATA: &str = "OTEL_PHP_CAPTURE_SENSITIVE_DATA";

thread_local! {
    static REQUEST_CAPTURE: Cell<Option<bool>> = const { Cell::new(None) };
}

static INVALID_VALUE_WARNED: AtomicBool = AtomicBool::new(false);

fn from_env() -> bool {
    match env::var(CAPTURE_SENSITIVE_DATA) {
        Ok(value) if value.eq_ignore_ascii_case("true") => true,
        Ok(value) if value.is_empty() || value.eq_ignore_ascii_case("false") => false,
        Ok(value) => {
            if !INVALID_VALUE_WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    "Invalid {CAPTURE_SENSITIVE_DATA} value {value:?}; only case-insensitive true enables sensitive capture, using false"
                );
            }
            false
        }
        Err(env::VarError::NotPresent) => false,
        Err(env::VarError::NotUnicode(_)) => {
            if !INVALID_VALUE_WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    "Invalid non-Unicode {CAPTURE_SENSITIVE_DATA} value; using false"
                );
            }
            false
        }
    }
}

pub fn begin_request() {
    REQUEST_CAPTURE.with(|capture| capture.set(Some(from_env())));
}

pub fn end_request() {
    REQUEST_CAPTURE.with(|capture| capture.set(None));
}

pub fn capture() -> bool {
    REQUEST_CAPTURE.with(|capture| match capture.get() {
        Some(value) => value,
        None => {
            let value = from_env();
            capture.set(Some(value));
            value
        }
    })
}

/// Preserve URL origin/path while removing userinfo, query, and fragment data
/// that commonly carries credentials or tokens.
pub fn sanitize_url(value: &str) -> String {
    sanitize_url_value(value, capture())
}

fn sanitize_url_value(value: &str, capture: bool) -> String {
    if capture {
        return value.to_string();
    }
    if let Ok(mut url) = url::Url::parse(value) {
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_query(None);
        url.set_fragment(None);
        return url.to_string();
    }
    value
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::sanitize_url_value;

    #[test]
    fn safe_urls_remove_userinfo_query_and_fragment() {
        assert_eq!(
            sanitize_url_value("https://user:pass@example.com/path?token=secret#part", false),
            "https://example.com/path"
        );
        assert_eq!(sanitize_url_value("/path?token=secret#part", false), "/path");
    }

    #[test]
    fn explicit_capture_preserves_the_original_url() {
        let url = "https://user:pass@example.com/path?token=secret#part";
        assert_eq!(sanitize_url_value(url, true), url);
    }
}
