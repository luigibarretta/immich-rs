#![forbid(unsafe_code)]
//! Domain types and pipeline contracts shared by all immich-rs frontends.

/// Human-readable component identity used by initial workspace smoke tests.
pub const COMPONENT: &str = "immich-rs-core";

#[cfg(test)]
mod tests {
    use super::COMPONENT;

    #[test]
    fn component_identity_is_stable() {
        assert_eq!(COMPONENT, "immich-rs-core");
    }
}
