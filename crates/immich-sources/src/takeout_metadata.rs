use immich_rs_core::{GeoCoordinates, NormalizedMetadata};
use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub const MAX_JSON_BYTES: u64 = 256 * 1_024;
const MAX_TITLE_BYTES: usize = 4_096;
const MAX_DESCRIPTION_BYTES: usize = 16 * 1_024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TakeoutDocument {
    pub title: String,
    pub metadata: NormalizedMetadata,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError {
    Invalid,
    Oversized,
    SourceChanged,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDocument {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    photo_taken_time: Option<RawTime>,
    #[serde(default)]
    creation_time: Option<RawTime>,
    #[serde(default)]
    geo_data_exif: Option<RawGeo>,
    #[serde(default)]
    geo_data: Option<RawGeo>,
}

#[derive(Deserialize)]
struct RawTime {
    timestamp: String,
}

#[derive(Clone, Copy, Deserialize)]
struct RawGeo {
    latitude: Option<f64>,
    longitude: Option<f64>,
}

pub fn parse_bytes(bytes: &[u8]) -> Result<TakeoutDocument, ParseError> {
    let raw: RawDocument = serde_json::from_slice(bytes).map_err(|_| ParseError::Invalid)?;
    validate_text(&raw.title, MAX_TITLE_BYTES, false)?;
    let description = raw
        .description
        .filter(|value| !value.is_empty())
        .map(|value| {
            validate_text(&value, MAX_DESCRIPTION_BYTES, true)?;
            Ok(value)
        })
        .transpose()?;
    let taken_at_utc = raw
        .photo_taken_time
        .or(raw.creation_time)
        .map(|value| normalize_timestamp(&value.timestamp))
        .transpose()?;
    let location = raw
        .geo_data_exif
        .or(raw.geo_data)
        .map(normalize_location)
        .transpose()?
        .flatten();
    Ok(TakeoutDocument {
        title: raw.title,
        metadata: NormalizedMetadata {
            description,
            taken_at_utc,
            location,
            albums: Vec::new(),
        },
    })
}

fn validate_text(value: &str, max_bytes: usize, allow_layout: bool) -> Result<(), ParseError> {
    let invalid_control = value
        .chars()
        .any(|character| character.is_control() && !(allow_layout && "\n\r\t".contains(character)));
    if value.is_empty()
        || value.len() > max_bytes
        || invalid_control
        || (!allow_layout && value.contains(['/', '\\']))
    {
        Err(ParseError::Invalid)
    } else {
        Ok(())
    }
}

fn normalize_timestamp(value: &str) -> Result<String, ParseError> {
    if value.is_empty() || value.bytes().any(|byte| !byte.is_ascii_digit()) {
        return Err(ParseError::Invalid);
    }
    let seconds = value.parse::<i64>().map_err(|_| ParseError::Invalid)?;
    OffsetDateTime::from_unix_timestamp(seconds)
        .map_err(|_| ParseError::Invalid)?
        .format(&Rfc3339)
        .map_err(|_| ParseError::Invalid)
}

fn normalize_location(value: RawGeo) -> Result<Option<GeoCoordinates>, ParseError> {
    let (Some(latitude), Some(longitude)) = (value.latitude, value.longitude) else {
        return Err(ParseError::Invalid);
    };
    if !latitude.is_finite()
        || !longitude.is_finite()
        || !(-90.0..=90.0).contains(&latitude)
        || !(-180.0..=180.0).contains(&longitude)
    {
        return Err(ParseError::Invalid);
    }
    if latitude == 0.0 && longitude == 0.0 {
        return Ok(None);
    }
    Ok(Some(GeoCoordinates {
        latitude: canonical_decimal(latitude),
        longitude: canonical_decimal(longitude),
    }))
}

fn canonical_decimal(value: f64) -> String {
    if value == 0.0 {
        "0".to_owned()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_metadata_is_normalized_without_formatted_time() -> Result<(), ParseError> {
        let document = parse_bytes(
            br#"{"title":"pixel.png","description":"synthetic\ntext","photoTakenTime":{"timestamp":"1704067200","formatted":"ignored"},"creationTime":{"timestamp":"0"},"geoData":{"latitude":1.25,"longitude":2.5},"geoDataExif":{"latitude":3.5,"longitude":-4.25}}"#,
        )?;
        assert_eq!(
            document.metadata.taken_at_utc.as_deref(),
            Some("2024-01-01T00:00:00Z")
        );
        assert_eq!(
            document.metadata.description.as_deref(),
            Some("synthetic\ntext")
        );
        let location = document.metadata.location.ok_or(ParseError::Invalid)?;
        assert_eq!(location.latitude, "3.5");
        assert_eq!(location.longitude, "-4.25");
        Ok(())
    }

    #[test]
    fn invalid_values_fail_and_zero_location_is_absent() -> Result<(), ParseError> {
        let zero = parse_bytes(br#"{"title":"pixel.png","geoData":{"latitude":0,"longitude":0}}"#)?;
        assert!(zero.metadata.location.is_none());
        assert!(parse_bytes(br#"{"title":"../pixel.png"}"#).is_err());
        assert!(
            parse_bytes(br#"{"title":"pixel.png","photoTakenTime":{"timestamp":"now"}}"#).is_err()
        );
        assert!(
            parse_bytes(br#"{"title":"pixel.png","geoData":{"latitude":91,"longitude":0}}"#)
                .is_err()
        );
        Ok(())
    }
}
