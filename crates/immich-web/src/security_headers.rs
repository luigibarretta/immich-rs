//! Web Console response header policy (ADR-0033, ADR-0036).
//!
//! The shared `http_hardening` layer writes these headers on every response,
//! rejections included, and replaces any value a handler set
//! (`ExistingHeaders::Overwrite`, the library default). The values are the
//! exact set the console sent before adopting the library: the CSP keeps
//! `form-action 'self'` (plus the OIDC issuer origin when the LAN profile is
//! active) and omits the library's image, font, manifest, worker
//! and object directives (`default-src 'none'` already denies them), the
//! Permissions-Policy denies only camera, microphone and geolocation, and no
//! HSTS header is sent.

use http_hardening::security_headers::{
    ContentSecurityPolicy, CrossOriginOpenerPolicy, CrossOriginResourcePolicy, FrameOptions,
    HeaderPolicyError, PermissionsPolicy, ReferrerPolicy, SecurityHeaders, SecurityHeadersLayer,
};

use crate::WebConfigError;

/// Validated layer for the console router, built once at startup.
///
/// `idp_issuer` is the validated OIDC issuer when the LAN profile is active:
/// `POST /oidc/login` is a form submission answered by a redirect to the
/// identity provider, and Chromium applies `form-action` to that redirect, so
/// the issuer origin (the only origin its endpoints may use) is allowed there.
pub fn layer(idp_issuer: Option<&str>) -> Result<SecurityHeadersLayer, WebConfigError> {
    let idp_origin = idp_issuer.map(issuer_origin).transpose()?;
    policy(idp_origin.as_deref())
        .map(SecurityHeadersLayer::new)
        .map_err(|_| WebConfigError::new("security header policy is invalid"))
}

fn issuer_origin(issuer: &str) -> Result<String, WebConfigError> {
    url::Url::parse(issuer)
        .ok()
        .filter(|url| url.scheme() == "https" && url.host().is_some())
        .map(|url| url.origin().ascii_serialization())
        .ok_or_else(|| WebConfigError::new("OIDC issuer is invalid"))
}

fn policy(idp_origin: Option<&str>) -> Result<SecurityHeaders, HeaderPolicyError> {
    let mut form_action = vec!["'self'"];
    form_action.extend(idp_origin);
    // `frame-ancestors 'none'` is appended last by the builder from
    // `FrameOptions::Deny`, matching the previous directive order.
    let csp = ContentSecurityPolicy::empty()
        .set("default-src", &["'none'"])?
        .set("style-src", &["'self'"])?
        .set("script-src", &["'self'"])?
        .set("connect-src", &["'self'"])?
        .set("form-action", &form_action)?
        .set("base-uri", &["'none'"])?;
    let permissions = PermissionsPolicy::empty()
        .deny("camera")?
        .deny("microphone")?
        .deny("geolocation")?;
    SecurityHeaders::builder()
        .hsts(None)
        .csp(Some(csp))
        .frame_options(FrameOptions::Deny)
        .nosniff(true)
        .referrer_policy(Some(ReferrerPolicy::NoReferrer))
        .permissions_policy(Some(permissions))
        .cross_origin_opener_policy(Some(CrossOriginOpenerPolicy::SameOrigin))
        .cross_origin_resource_policy(Some(CrossOriginResourcePolicy::SameOrigin))
        .header("cache-control", "private, no-store, max-age=0")
        .header("pragma", "no-cache")
        .header("x-robots-tag", "noindex, nofollow")
        .build()
}

#[cfg(test)]
mod tests {
    use super::{issuer_origin, policy};

    fn csp(idp_origin: Option<&str>) -> String {
        policy(idp_origin)
            .ok()
            .and_then(|headers| {
                headers
                    .headers_str()
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-security-policy"))
                    .map(|(_, value)| value.to_owned())
            })
            .unwrap_or_default()
    }

    #[test]
    fn form_action_allows_only_self_without_oidc() {
        assert!(csp(None).contains("form-action 'self';"));
    }

    #[test]
    fn form_action_adds_exactly_the_issuer_origin_with_oidc() {
        let origin = issuer_origin("https://auth.example.test/application/o/immich/");
        assert_eq!(origin.as_deref().ok(), Some("https://auth.example.test"));
        assert!(
            csp(Some("https://auth.example.test"))
                .contains("form-action 'self' https://auth.example.test;")
        );
        assert!(issuer_origin("http://auth.example.test/").is_err());
    }
}
