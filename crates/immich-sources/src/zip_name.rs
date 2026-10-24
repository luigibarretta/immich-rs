//! ZIP entry-name decoding that keeps the established portable-path contract.
//!
//! zip 9 decodes an entry name as UTF-8 whenever its bytes are valid UTF-8 and
//! as IBM code page 437 otherwise, whatever the entry's language-encoding flag
//! says. Plans, checkpoints and import staging compare portable paths derived
//! from these names, so this module keeps the rule the scanner has always
//! used: flag bit 11 set means UTF-8 with lossy replacement, otherwise every
//! byte is code page 437. zip sets bit 11 itself after it applies a
//! CRC-verified Info-ZIP Unicode Path extra field, so that field still wins.

use std::borrow::Cow;
use std::io::Read;
use std::path::{Component, PathBuf};

use zip::read::ZipFile;

/// General-purpose flag bit 11: the name and comment are UTF-8.
const LANGUAGE_ENCODING_FLAG: u16 = 1 << 11;

/// Code page 437 characters for the bytes 0x80 through 0xFF, in order.
const CP437_HIGH_HALF: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»\
░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀\
αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

/// Decode one entry name: UTF-8 (lossy) when flagged, code page 437 otherwise.
#[must_use]
pub fn zip_entry_name<'a, R: Read>(entry: &'a ZipFile<'_, R>) -> Cow<'a, str> {
    let raw = entry.name_raw();
    if raw.is_ascii() || entry.flags().as_u16() & LANGUAGE_ENCODING_FLAG != 0 {
        String::from_utf8_lossy(raw)
    } else {
        Cow::Owned(raw.iter().map(|&byte| cp437_char(byte)).collect())
    }
}

/// Return the enclosed relative path of `name`, the entry's decoded name.
///
/// zip's own `enclosed_name` decides whether the name is safe; it works on
/// zip's decoding, which differs from `name` only in non-ASCII characters.
/// Every structural character (`/`, `\`, `:`, `.`, NUL) is ASCII and both
/// decodings map ASCII bytes to themselves, so the component boundaries are
/// identical. The components are then taken from `name`, and any mismatch in
/// that correspondence fails closed with `None`.
#[must_use]
pub fn zip_enclosed_name<R: Read>(entry: &ZipFile<'_, R>, name: &str) -> Option<PathBuf> {
    let enclosed = entry.enclosed_name()?;
    let current = entry.name().ok()?;
    if current == name {
        return Some(enclosed);
    }
    let mut legacy_segments = name.split('/').filter(|segment| !segment.is_empty()).rev();
    let mut current_segments = current
        .split('/')
        .filter(|segment| !segment.is_empty())
        .rev();
    let mut components = Vec::new();
    for component in enclosed.components().rev() {
        let Component::Normal(value) = component else {
            return None;
        };
        let value = value.to_str()?;
        let (legacy, current) = (legacy_segments.next()?, current_segments.next()?);
        let prefix_len = current.len().checked_sub(value.len())?;
        let prefix = current.get(..prefix_len)?;
        if !current.ends_with(value) || !prefix.is_ascii() || legacy.get(..prefix_len)? != prefix {
            return None;
        }
        components.push(legacy.get(prefix_len..)?);
    }
    if legacy_segments.count() != current_segments.count() {
        return None;
    }
    Some(components.into_iter().rev().collect())
}

fn cp437_char(byte: u8) -> char {
    if byte.is_ascii() {
        char::from(byte)
    } else {
        CP437_HIGH_HALF
            .chars()
            .nth(usize::from(byte - 0x80))
            .unwrap_or(char::REPLACEMENT_CHARACTER)
    }
}

#[cfg(test)]
mod tests {
    use super::CP437_HIGH_HALF;

    #[test]
    fn the_code_page_437_table_covers_the_whole_high_half() {
        assert_eq!(CP437_HIGH_HALF.chars().count(), 128);
        assert!(CP437_HIGH_HALF.chars().all(|value| !value.is_ascii()));
    }
}
