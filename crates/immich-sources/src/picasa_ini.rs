use std::collections::BTreeMap;

const MAX_INI_BYTES: usize = 1_048_576;
const MAX_LINES: usize = 20_000;
const MAX_LINE_BYTES: usize = 16_384;
const MAX_SECTIONS: usize = 10_000;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PicasaDocument {
    pub album: Option<String>,
    pub captions: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IniError {
    Oversized,
    InvalidUtf8,
    InvalidSyntax,
    Conflict,
}

pub fn parse(bytes: &[u8]) -> Result<PicasaDocument, IniError> {
    if bytes.len() > MAX_INI_BYTES {
        return Err(IniError::Oversized);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| IniError::InvalidUtf8)?;
    let mut document = PicasaDocument::default();
    let mut section = None::<String>;
    let mut sections = 0_usize;
    for (index, raw_line) in text.lines().enumerate() {
        if index >= MAX_LINES || raw_line.len() > MAX_LINE_BYTES {
            return Err(IniError::Oversized);
        }
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with([';', '#']) {
            continue;
        }
        if line.starts_with('[') {
            let Some(name) = line
                .strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
            else {
                return Err(IniError::InvalidSyntax);
            };
            if name.is_empty() || name.len() > 4_096 || name.chars().any(char::is_control) {
                return Err(IniError::InvalidSyntax);
            }
            sections = sections.saturating_add(1);
            if sections > MAX_SECTIONS {
                return Err(IniError::Oversized);
            }
            section = Some(name.to_owned());
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(IniError::InvalidSyntax);
        };
        let (key, value) = (key.trim(), value.trim());
        if key.is_empty() || key.len() > 256 || value.len() > 16_384 {
            return Err(IniError::Oversized);
        }
        let Some(section) = section.as_deref() else {
            return Err(IniError::InvalidSyntax);
        };
        if section.eq_ignore_ascii_case("Picasa") && key.eq_ignore_ascii_case("name") {
            assign(&mut document.album, value)?;
        } else if key.eq_ignore_ascii_case("caption") {
            let entry = document.captions.entry(section.to_owned()).or_default();
            if !entry.is_empty() && entry != value {
                return Err(IniError::Conflict);
            }
            if value.is_empty()
                || value
                    .chars()
                    .any(|character| character.is_control() && !"\n\r\t".contains(character))
            {
                return Err(IniError::InvalidSyntax);
            }
            entry.clear();
            entry.push_str(value);
        }
    }
    Ok(document)
}

fn assign(target: &mut Option<String>, value: &str) -> Result<(), IniError> {
    if value.is_empty()
        || value.len() > 4_096
        || value.contains(['/', '\\'])
        || value.chars().any(char::is_control)
    {
        return Err(IniError::InvalidSyntax);
    }
    if target.as_deref().is_some_and(|existing| existing != value) {
        return Err(IniError::Conflict);
    }
    *target = Some(value.to_owned());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{IniError, parse};

    #[test]
    fn parses_only_album_and_caption() -> Result<(), IniError> {
        let document = parse(
            b"[Picasa]\nname=Synthetic Album\nfoo=ignored\n[image.png]\ncaption=A caption\nstar=yes\n",
        )?;
        assert_eq!(document.album.as_deref(), Some("Synthetic Album"));
        assert_eq!(
            document.captions.get("image.png").map(String::as_str),
            Some("A caption")
        );
        Ok(())
    }

    #[test]
    fn conflicting_album_fails_closed() {
        assert_eq!(
            parse(b"[Picasa]\nname=First\nname=Second\n"),
            Err(IniError::Conflict)
        );
    }
}
