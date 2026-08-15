use serde::{Deserialize, Serialize};

/// Canonical geographic coordinates without binary floating-point output.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeoCoordinates {
    /// Canonical decimal latitude in the inclusive range -90..=90.
    pub latitude: String,
    /// Canonical decimal longitude in the inclusive range -180..=180.
    pub longitude: String,
}

impl GeoCoordinates {
    pub(crate) fn is_valid(&self) -> bool {
        decimal_in_range(&self.latitude, -90.0, 90.0)
            && decimal_in_range(&self.longitude, -180.0, 180.0)
    }
}

/// Source-neutral metadata resolved for one candidate asset.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedMetadata {
    /// Bounded source description, when present.
    pub description: Option<String>,
    /// Capture instant normalized to RFC 3339 UTC at second precision.
    pub taken_at_utc: Option<String>,
    /// Valid non-sentinel source location, when present.
    pub location: Option<GeoCoordinates>,
    /// Strictly sorted and deduplicated album titles.
    pub albums: Vec<String>,
}

impl NormalizedMetadata {
    pub(crate) fn is_valid(&self) -> bool {
        self.description
            .as_ref()
            .is_none_or(|value| !value.contains('\0') && value.len() <= 16 * 1_024)
            && self.taken_at_utc.as_ref().is_none_or(|value| {
                value.len() == 20 && value.ends_with('Z') && value.as_bytes().get(10) == Some(&b'T')
            })
            && self.location.as_ref().is_none_or(GeoCoordinates::is_valid)
            && self.albums.iter().all(|album| {
                !album.is_empty() && album.len() <= 4_096 && !album.contains(['\0', '/', '\\'])
            })
            && self.albums.windows(2).all(|pair| pair[0] < pair[1])
    }
}

fn decimal_in_range(value: &str, minimum: f64, maximum: f64) -> bool {
    value
        .parse::<f64>()
        .is_ok_and(|number| number.is_finite() && (minimum..=maximum).contains(&number))
}
