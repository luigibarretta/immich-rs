use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

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
            .is_none_or(|value| valid_text(value, 16 * 1_024, true))
            && self.taken_at_utc.as_ref().is_none_or(|value| {
                OffsetDateTime::parse(value, &Rfc3339)
                    .ok()
                    .and_then(|instant| instant.format(&Rfc3339).ok())
                    .is_some_and(|canonical| canonical == *value && value.ends_with('Z'))
            })
            && self.location.as_ref().is_none_or(GeoCoordinates::is_valid)
            && self
                .albums
                .iter()
                .all(|album| valid_text(album, 4_096, false))
            && self.albums.windows(2).all(|pair| pair[0] < pair[1])
    }
}

fn decimal_in_range(value: &str, minimum: f64, maximum: f64) -> bool {
    value.parse::<f64>().is_ok_and(|number| {
        let canonical = if number == 0.0 {
            "0".to_owned()
        } else {
            number.to_string()
        };
        number.is_finite() && (minimum..=maximum).contains(&number) && value == canonical
    })
}

fn valid_text(value: &str, max_bytes: usize, allow_layout: bool) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && !value.chars().any(|character| {
            character.is_control() && !(allow_layout && "\n\r\t".contains(character))
        })
        && (allow_layout || !value.contains(['/', '\\']))
}
