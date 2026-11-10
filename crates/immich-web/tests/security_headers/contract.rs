use axum::http::HeaderMap;

/// The complete Web Console response header policy (ADR-0033, ADR-0036).
/// Every response, including rejections, carries exactly these values once.
pub const EXPECTED: [(&str, &str); 10] = [
    ("cache-control", "private, no-store, max-age=0"),
    ("pragma", "no-cache"),
    (
        "content-security-policy",
        "default-src 'none'; style-src 'self'; script-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
    ),
    ("x-frame-options", "DENY"),
    ("referrer-policy", "no-referrer"),
    ("x-content-type-options", "nosniff"),
    (
        "permissions-policy",
        "camera=(), microphone=(), geolocation=()",
    ),
    ("cross-origin-opener-policy", "same-origin"),
    ("cross-origin-resource-policy", "same-origin"),
    ("x-robots-tag", "noindex, nofollow"),
];

/// Headers the console deliberately does not send: HSTS is not part of the
/// policy (loopback HTTP and direct LAN TLS alike), nor is a report-only CSP.
const ABSENT: [&str; 2] = [
    "strict-transport-security",
    "content-security-policy-report-only",
];

/// Assert the exact policy on one response; `context` names the case.
pub fn assert_security_headers(headers: &HeaderMap, context: &str) {
    for (name, value) in EXPECTED {
        let values: Vec<_> = headers
            .get_all(name)
            .iter()
            .map(|item| item.to_str().unwrap_or("<non-ascii>"))
            .collect();
        assert_eq!(values, [value], "{context}: header {name}");
    }
    for name in ABSENT {
        assert!(!headers.contains_key(name), "{context}: unexpected {name}");
    }
}
