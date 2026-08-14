#![forbid(unsafe_code)]
//! Source adapters for folders, archives and export formats.

/// Human-readable component identity used by initial workspace smoke tests.
pub const COMPONENT: &str = "immich-rs-sources";

#[cfg(test)]
mod tests {
    use super::COMPONENT;

    #[test]
    fn component_identity_is_stable() {
        assert_eq!(COMPONENT, "immich-rs-sources");
    }
}
