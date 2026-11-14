//! Saved embed appearance for one blog.
//!
//! Owners style their embedded posts through typed tokens (colors, sizes,
//! enum choices), never raw CSS text. The value is stored as validated JSON
//! on the blog row, shipped through the management and public delivery
//! APIs, and turned into custom properties by embed v2. `None` means "use
//! the built-in appearance", so unset blogs keep the stock look.

use crate::error::Error;

/// The only schema version this code understands; anything else is rejected
/// at the write edge so future formats can evolve independently.
pub const APPEARANCE_VERSION: u32 = 1;

/// Inclusive bounds for the numeric tokens. Sizes are percentages (100 = the
/// built-in value); layout and decoration are pixels.
pub const BODY_SIZE_RANGE: (u16, u16) = (75, 200);
pub const HEADING_SCALE_RANGE: (u16, u16) = (100, 225);
pub const LINE_HEIGHT_RANGE: (u16, u16) = (100, 250);
pub const MAX_WIDTH_RANGE: (u16, u16) = (320, 1200);
pub const PADDING_RANGE: (u16, u16) = (0, 96);
pub const SPACING_RANGE: (u16, u16) = (0, 96);
pub const BORDER_WIDTH_RANGE: (u8, u8) = (0, 8);
pub const CORNER_RADIUS_RANGE: (u8, u8) = (0, 32);

/// A saved appearance document. Every group is optional: an owner can set
/// just a link color. Unknown fields are rejected by serde, so the stored
/// and delivered shapes never carry dead data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbedAppearance {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<ColorGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typography: Option<TypographyGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoration: Option<DecorationGroup>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorGroup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Hex or the literal `transparent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypographyGroup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<FontChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_scale: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontChoice {
    /// Predefined system sans-serif stack.
    Sans,
    /// Predefined system serif stack.
    Serif,
    /// Predefined system monospace stack.
    Mono,
    /// The host page's font family.
    Inherit,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutGroup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecorationGroup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_width: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_radius: Option<u8>,
}

impl EmbedAppearance {
    /// Validates and normalizes in place: version checked, colors canonical
    /// (lowercase `#rrggbb`), numeric ranges enforced. Storage and delivery
    /// only ever see the normalized form.
    pub fn validate(&mut self) -> Result<(), Error> {
        if self.version != APPEARANCE_VERSION {
            return Err(Error::InvalidInput);
        }
        if let Some(colors) = self.colors.as_mut() {
            if let Some(text) = colors.text.as_mut() {
                *text = hex_color(text)?;
            }
            if let Some(background) = colors.background.as_mut() {
                *background = background_color(background)?;
            }
            if let Some(link) = colors.link.as_mut() {
                *link = hex_color(link)?;
            }
            if let Some(muted) = colors.muted.as_mut() {
                *muted = hex_color(muted)?;
            }
            if let Some(border) = colors.border.as_mut() {
                *border = hex_color(border)?;
            }
        }
        if let Some(typography) = self.typography.as_mut() {
            bound(&mut typography.body_size, BODY_SIZE_RANGE)?;
            bound(&mut typography.heading_scale, HEADING_SCALE_RANGE)?;
            bound(&mut typography.line_height, LINE_HEIGHT_RANGE)?;
        }
        if let Some(layout) = self.layout.as_mut() {
            bound(&mut layout.max_width, MAX_WIDTH_RANGE)?;
            bound(&mut layout.padding, PADDING_RANGE)?;
            bound(&mut layout.spacing, SPACING_RANGE)?;
        }
        if let Some(decoration) = self.decoration.as_mut() {
            bound(&mut decoration.border_width, BORDER_WIDTH_RANGE)?;
            bound(&mut decoration.corner_radius, CORNER_RADIUS_RANGE)?;
        }
        Ok(())
    }
}

/// Parses a stored appearance value defensively: unparseable, unsupported or
/// out-of-range stored JSON reads as unset, so one bad row can never break
/// delivery of the blog itself.
pub fn parse_saved(raw: &str) -> Option<EmbedAppearance> {
    let mut value: EmbedAppearance = serde_json::from_str(raw).ok()?;
    if value.version != APPEARANCE_VERSION {
        return None;
    }
    value.validate().ok()?;
    Some(value)
}

/// Serializes a validated appearance for storage. Infallible in practice for
/// this all-plain-data shape, but mapped instead of unwrapped.
pub fn to_json(value: &EmbedAppearance) -> Result<String, Error> {
    serde_json::to_string(value).map_err(|_| {
        Error::Internal(crate::error::InternalError::new(std::io::Error::other(
            "appearance serialize",
        )))
    })
}

fn bound<T: PartialOrd>(value: &mut Option<T>, range: (T, T)) -> Result<(), Error> {
    if let Some(inner) = value
        && !(range.0..=range.1).contains(inner) {
            return Err(Error::InvalidInput);
        }
    Ok(())
}

fn hex_color(input: &str) -> Result<String, Error> {
    let hex = input.strip_prefix('#').ok_or(Error::InvalidInput)?;
    let expanded = match hex.len() {
        3 => hex.chars().flat_map(|ch| [ch, ch]).collect::<String>(),
        6 => hex.to_owned(),
        _ => return Err(Error::InvalidInput),
    };
    if !expanded.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(Error::InvalidInput);
    }
    Ok(format!("#{expanded}").to_ascii_lowercase())
}

fn background_color(input: &str) -> Result<String, Error> {
    if input.eq_ignore_ascii_case("transparent") {
        return Ok("transparent".to_owned());
    }
    hex_color(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn appearance(body: serde_json::Value) -> EmbedAppearance {
        serde_json::from_value(body).expect("deserialize appearance")
    }

    #[test]
    fn valid_documents_normalize_colors_and_pass() {
        let mut value = appearance(json!({
            "version": 1,
            "colors": {
                "text": "#1A2B3C",
                "background": "#abc",
                "link": "#7C3AED",
                "muted": "#666666",
                "border": "#DDD"
            },
            "typography": { "font": "serif", "body_size": 110, "line_height": 180 },
            "layout": { "max_width": 720, "padding": 24 },
            "decoration": { "border_width": 1, "corner_radius": 8 }
        }));
        value.validate().expect("valid appearance");
        assert_eq!(
            value.colors.as_ref().unwrap().text.as_deref(),
            Some("#1a2b3c")
        );
        assert_eq!(
            value.colors.as_ref().unwrap().background.as_deref(),
            Some("#aabbcc")
        );
        assert_eq!(
            value.typography.as_ref().unwrap().font,
            Some(FontChoice::Serif)
        );
    }

    #[test]
    fn transparent_background_is_kept() {
        let mut value = appearance(json!({
            "version": 1,
            "colors": { "background": "Transparent" }
        }));
        value.validate().expect("valid appearance");
        assert_eq!(
            value.colors.as_ref().unwrap().background.as_deref(),
            Some("transparent")
        );
    }

    #[test]
    fn unsupported_version_and_ranges_are_rejected() {
        let mut value = appearance(json!({ "version": 2 }));
        assert!(value.validate().is_err());

        let mut value = appearance(json!({
            "version": 1,
            "typography": { "body_size": 500 }
        }));
        assert!(value.validate().is_err());

        let mut value = appearance(json!({
            "version": 1,
            "layout": { "max_width": 100 }
        }));
        assert!(value.validate().is_err());

        let mut value = appearance(json!({
            "version": 1,
            "decoration": { "border_width": 9 }
        }));
        assert!(value.validate().is_err());
    }

    #[test]
    fn malformed_colors_are_rejected() {
        for bad in ["1a2b3c", "#12345", "#1234567", "#12g45z", ""] {
            let mut value = appearance(json!({
                "version": 1,
                "colors": { "link": bad }
            }));
            assert!(value.validate().is_err(), "should reject {bad:?}");
        }
        let mut value = appearance(json!({
            "version": 1,
            "colors": { "background": "url(https://host/x.gif)" }
        }));
        assert!(value.validate().is_err());
    }

    #[test]
    fn unknown_fields_are_rejected_at_deserialization() {
        assert!(
            serde_json::from_value::<EmbedAppearance>(json!({
                "version": 1,
                "css": "body { display: none }"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<EmbedAppearance>(json!({
                "version": 1,
                "colors": { "shadow": "#000" }
            }))
            .is_err()
        );
    }

    #[test]
    fn parse_saved_drops_unparseable_or_unsupported_values() {
        assert!(parse_saved("not json").is_none());
        assert!(parse_saved(r#"{"version": 9}"#).is_none());
        let mut valid = appearance(json!({
            "version": 1,
            "colors": { "link": "#7c3aed" }
        }));
        valid.validate().unwrap();
        let saved = parse_saved(&to_json(&valid).unwrap()).expect("saved value parses");
        assert_eq!(saved, valid);
    }
}
