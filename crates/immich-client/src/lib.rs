#![forbid(unsafe_code)]
//! Version-aware Immich HTTP API boundary.

/// Human-readable component identity used by initial workspace smoke tests.
pub const COMPONENT: &str = "immich-rs-client";

#[cfg(test)]
mod tests {
    use super::COMPONENT;

    #[test]
    fn component_identity_is_stable() {
        assert_eq!(COMPONENT, "immich-rs-client");
    }
}
