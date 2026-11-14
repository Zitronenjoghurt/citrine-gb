use axum::body::Bytes;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::Response;
use std::collections::HashSet;
use std::sync::Mutex;

pub const REPORTS_PATH: &str = "/isolation-reports";
const REPORTS_KEPT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Isolation {
    Off,
    #[default]
    Report,
    On,
}

impl Isolation {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "report" => Some(Self::Report),
            "on" => Some(Self::On),
            _ => None,
        }
    }

    pub fn headers(self) -> Vec<(HeaderName, HeaderValue)> {
        let header = |name: &'static str, value: &'static str| {
            (
                HeaderName::from_static(name),
                HeaderValue::from_static(value),
            )
        };
        match self {
            Self::Off => Vec::new(),
            Self::Report => vec![
                header("reporting-endpoints", "isolation=\"/isolation-reports\""),
                header(
                    "cross-origin-opener-policy-report-only",
                    "same-origin; report-to=\"isolation\"",
                ),
                header(
                    "cross-origin-embedder-policy-report-only",
                    "require-corp; report-to=\"isolation\"",
                ),
            ],
            Self::On => vec![
                header("cross-origin-opener-policy", "same-origin"),
                header("cross-origin-embedder-policy", "require-corp"),
                header("cross-origin-resource-policy", "same-origin"),
            ],
        }
    }

    pub fn add_headers(self, response: &mut Response) {
        response.headers_mut().extend(self.headers());
    }
}

#[derive(Default)]
pub struct Reports {
    seen: Mutex<HashSet<String>>,
}

impl Reports {
    pub fn take(&self, body: &Bytes) -> StatusCode {
        let Ok(reports) = serde_json::from_slice::<Vec<serde_json::Value>>(body) else {
            return StatusCode::BAD_REQUEST;
        };
        for report in reports {
            let line = describe(&report);
            let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.len() < REPORTS_KEPT && seen.insert(line.clone()) {
                log::warn!("isolation report: {line}");
            }
        }
        StatusCode::NO_CONTENT
    }
}

fn describe(report: &serde_json::Value) -> String {
    let text = |value: &serde_json::Value, key: &str| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .chars()
            .take(200)
            .collect::<String>()
    };
    let body = report.get("body").cloned().unwrap_or_default();
    let blocked = ["blockedURL", "previousResponseURL", "nextResponseURL"]
        .into_iter()
        .map(|key| text(&body, key))
        .find(|url| url != "-")
        .unwrap_or_else(|| "-".to_string());
    format!(
        "{} {} on {} blocked {}",
        text(report, "type"),
        text(&body, "type"),
        text(report, "url"),
        blocked
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(isolation: Isolation) -> Vec<String> {
        isolation
            .headers()
            .into_iter()
            .map(|(name, _)| name.to_string())
            .collect()
    }

    #[test]
    fn report_sends_only_report_only_headers_and_on_sends_all_three() {
        assert!(names(Isolation::Off).is_empty());
        assert_eq!(
            names(Isolation::Report),
            [
                "reporting-endpoints",
                "cross-origin-opener-policy-report-only",
                "cross-origin-embedder-policy-report-only"
            ]
        );
        assert_eq!(
            names(Isolation::On),
            [
                "cross-origin-opener-policy",
                "cross-origin-embedder-policy",
                "cross-origin-resource-policy"
            ]
        );
    }

    #[test]
    fn the_endpoint_header_names_the_reports_path() {
        let (_, value) = &Isolation::Report.headers()[0];
        assert!(value.to_str().unwrap().contains(REPORTS_PATH));
    }

    #[test]
    fn parse_reads_the_three_words_only() {
        assert_eq!(Isolation::parse(" On "), Some(Isolation::On));
        assert_eq!(Isolation::parse("report"), Some(Isolation::Report));
        assert_eq!(Isolation::parse("off"), Some(Isolation::Off));
        assert_eq!(Isolation::parse("true"), None);
    }

    #[test]
    fn a_report_names_its_kind_page_and_blocked_address_once() {
        let reports = Reports::default();
        let body = Bytes::from(
            r#"[{"type":"coep","url":"https://gb.example.com/","body":{"type":"corp","blockedURL":"https://cdn.example.net/a.png","disposition":"reporting"}}]"#,
        );
        assert_eq!(reports.take(&body), StatusCode::NO_CONTENT);
        assert_eq!(reports.take(&body), StatusCode::NO_CONTENT);
        let seen = reports.seen.lock().unwrap();
        assert_eq!(
            seen.iter().collect::<Vec<_>>(),
            ["coep corp on https://gb.example.com/ blocked https://cdn.example.net/a.png"]
        );
    }

    #[test]
    fn a_body_that_is_not_a_report_list_is_refused() {
        assert_eq!(
            Reports::default().take(&Bytes::from_static(b"{}")),
            StatusCode::BAD_REQUEST
        );
    }
}
