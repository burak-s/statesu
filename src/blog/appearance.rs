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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<MarkdownGroup>,
}

/// Element-specific styles; colors are hex, dimensions are pixels.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkdownGroup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h1_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h2_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h3_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h4_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h5_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h6_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h1_weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h2_weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h3_weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h4_weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h5_weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h6_weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h1_spacing: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h2_spacing: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h3_spacing: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h4_spacing: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h5_spacing: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h6_spacing: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_decoration: Option<LinkDecoration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_hover_decoration: Option<LinkDecoration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_hover_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_border_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_border_width: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_radius: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_code_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_code_padding: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_code_radius: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_code_border_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_code_border_width: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_header_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_stripe_background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_width: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_align: Option<ImageAlign>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_spacing: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paragraph_spacing: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_spacing: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_indent: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_spacing: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_padding: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_code_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_code_background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_border_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_border_width: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_padding: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_header_background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_border_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_cell_padding: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_radius: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_width: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkDecoration {
    Underline,
    None,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageAlign {
    Left,
    Center,
    Right,
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
    pub body_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_scale: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<u16>,
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
    /// True when no group is set: the keyword-defaulted write edge turns
    /// such documents into "no saved appearance".
    pub fn is_default(&self) -> bool {
        self.colors.is_none()
            && self.typography.is_none()
            && self.layout.is_none()
            && self.decoration.is_none()
            && self.markdown.is_none()
    }

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
        if let Some(markdown) = self.markdown.as_mut() {
            for color in [
                &mut markdown.code_color,
                &mut markdown.code_background,
                &mut markdown.inline_code_color,
                &mut markdown.inline_code_background,
                &mut markdown.quote_color,
                &mut markdown.quote_background,
                &mut markdown.quote_border_color,
                &mut markdown.table_header_background,
                &mut markdown.table_border_color,
                &mut markdown.rule_color,
                &mut markdown.link_hover_color,
                &mut markdown.code_border_color,
                &mut markdown.inline_code_border_color,
                &mut markdown.table_header_color,
                &mut markdown.table_stripe_background,
            ]
            .into_iter()
            .flatten()
            {
                *color = hex_color(color)?;
            }
            for value in [
                &mut markdown.paragraph_spacing,
                &mut markdown.heading_spacing,
                &mut markdown.list_indent,
                &mut markdown.list_spacing,
                &mut markdown.code_padding,
                &mut markdown.quote_padding,
                &mut markdown.table_cell_padding,
                &mut markdown.inline_code_padding,
                &mut markdown.image_spacing,
                &mut markdown.h1_spacing,
                &mut markdown.h2_spacing,
                &mut markdown.h3_spacing,
                &mut markdown.h4_spacing,
                &mut markdown.h5_spacing,
                &mut markdown.h6_spacing,
            ] {
                bound(value, SPACING_RANGE)?;
            }
            bound(&mut markdown.quote_border_width, BORDER_WIDTH_RANGE)?;
            bound(&mut markdown.rule_width, BORDER_WIDTH_RANGE)?;
            bound(&mut markdown.image_radius, CORNER_RADIUS_RANGE)?;
            bound(&mut markdown.code_radius, CORNER_RADIUS_RANGE)?;
            bound(&mut markdown.inline_code_radius, CORNER_RADIUS_RANGE)?;
            bound(&mut markdown.code_border_width, BORDER_WIDTH_RANGE)?;
            bound(&mut markdown.inline_code_border_width, BORDER_WIDTH_RANGE)?;
            bound(&mut markdown.image_width, (1, 100))?;
            bound(&mut markdown.code_size, (8, 48))?;
            bound(&mut markdown.inline_code_size, (8, 48))?;
            for size in [
                &mut markdown.h1_size,
                &mut markdown.h2_size,
                &mut markdown.h3_size,
                &mut markdown.h4_size,
                &mut markdown.h5_size,
                &mut markdown.h6_size,
            ] {
                bound(size, (12, 96))?;
            }
            for weight in [
                &mut markdown.h1_weight,
                &mut markdown.h2_weight,
                &mut markdown.h3_weight,
                &mut markdown.h4_weight,
                &mut markdown.h5_weight,
                &mut markdown.h6_weight,
            ] {
                bound(weight, (100, 900))?;
            }
        }
        Ok(())
    }
}

/// Parses a stored appearance value defensively: unparseable, unsupported or
/// out-of-range stored JSON reads as unset, so one bad row can never break
/// delivery of the blog itself.
pub fn parse_saved(raw: &str) -> Option<EmbedAppearance> {
    let mut stored: serde_json::Value = serde_json::from_str(raw).ok()?;
    // Retired font choices must not discard a blog's other saved styling.
    if let Some(typography) = stored
        .get_mut("typography")
        .and_then(serde_json::Value::as_object_mut)
    {
        typography.remove("font");
        typography.remove("google_font");
    }
    let mut value: EmbedAppearance = serde_json::from_value(stored).ok()?;
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
        && !(range.0..=range.1).contains(inner)
    {
        return Err(Error::InvalidInput);
    }
    Ok(())
}

fn hex_color(input: &str) -> Result<String, Error> {
    let lowered = input.trim().to_ascii_lowercase();
    if let Some(hex) = lowered.strip_prefix('#') {
        let expanded = match hex.len() {
            3 => hex.chars().flat_map(|ch| [ch, ch]).collect::<String>(),
            4 => hex.chars().flat_map(|ch| [ch, ch]).collect::<String>(),
            6 => hex.to_owned(),
            8 => hex.to_owned(),
            _ => return Err(Error::InvalidInput),
        };
        if !expanded.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput);
        }
        return Ok(format!("#{expanded}"));
    }
    let (body, alpha) = lowered
        .strip_prefix("rgba(")
        .and_then(|body| body.strip_suffix(')'))
        .map(|body| (body, true))
        .or_else(|| {
            lowered
                .strip_prefix("rgb(")
                .and_then(|body| body.strip_suffix(')'))
                .map(|body| (body, false))
        })
        .ok_or(Error::InvalidInput)?;
    let parts: Vec<&str> = body.split(',').collect();
    if parts.len() != if alpha { 4 } else { 3 } {
        return Err(Error::InvalidInput);
    }
    let mut channels = Vec::with_capacity(4);
    for part in &parts[..3] {
        channels.push(part.trim().parse::<u8>().map_err(|_| Error::InvalidInput)?);
    }
    channels.push(if alpha {
        let value: f64 = parts[3].trim().parse().map_err(|_| Error::InvalidInput)?;
        if !(0.0..=1.0).contains(&value) {
            return Err(Error::InvalidInput);
        }
        (value * 255.0).round() as u8
    } else {
        255
    });
    Ok(format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        channels[0], channels[1], channels[2], channels[3]
    ))
}

/// Color keywords that mean "keep the built-in or website-inherited look".
const COLOR_SENTINELS: [&str; 3] = ["inherit", "revert", "currentcolor"];

/// Drops keyword defaults from an appearance document at the write edge:
/// `inherit`/`revert`/`currentcolor` (and `transparent` for Markdown groups)
/// are the spelling of "use the default", so they are removed before strict
/// validation instead of being stored. Emptied groups stay as empty objects;
/// a document carrying only empty groups means "the default everywhere".
pub fn strip_default_sentinels(document: &mut serde_json::Value) {
    let Some(groups) = document.as_object_mut() else {
        return;
    };
    for (group, fields) in groups.iter_mut() {
        if group == "version" {
            continue;
        }
        let Some(map) = fields.as_object_mut() else {
            continue;
        };
        map.retain(|key, value| {
            let Some(text) = value.as_str() else {
                return true;
            };
            let text = text.trim().to_ascii_lowercase();
            if group == "colors" {
                !COLOR_SENTINELS.contains(&text.as_str())
            } else if group == "markdown" {
                !(COLOR_SENTINELS.contains(&text.as_str()) || text == "transparent")
            } else {
                text != "revert" && !(text == "none" && key == "max_width")
            }
        });
    }
}

/// True when a document would carry no styling at all: a valid version plus
/// only empty groups. The CLI treats such a file as "clear the appearance".
pub fn is_default_document(document: &serde_json::Value) -> bool {
    document.as_object().is_some_and(|groups| {
        groups.get("version").and_then(serde_json::Value::as_u64)
            == Some(u64::from(APPEARANCE_VERSION))
            && groups.iter().all(|(key, value)| {
                key == "version" || value.as_object().is_some_and(|fields| fields.is_empty())
            })
    })
}

/// Removes groups left empty by [`strip_default_sentinels`]; called after
/// the emptiness check so "the default everywhere" stays recognizable.
pub fn drop_empty_groups(document: &mut serde_json::Value) {
    let Some(groups) = document.as_object_mut() else {
        return;
    };
    let emptied: Vec<String> = groups
        .iter()
        .filter(|(key, value)| {
            key.as_str() != "version" && value.as_object().is_some_and(|fields| fields.is_empty())
        })
        .map(|(key, _)| key.clone())
        .collect();
    for name in emptied {
        groups.remove(&name);
    }
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

    #[test]
    fn markdown_tokens_validate_and_normalize() {
        for key in [
            "code_color",
            "code_background",
            "inline_code_color",
            "inline_code_background",
            "quote_color",
            "quote_background",
            "quote_border_color",
            "table_header_background",
            "table_border_color",
            "rule_color",
            "link_hover_color",
            "code_border_color",
            "table_header_color",
            "table_stripe_background",
            "inline_code_border_color",
        ] {
            let mut value = appearance(json!({ "version": 1, "markdown": { key: "#ABC" } }));
            value.validate().unwrap();
            assert_eq!(
                serde_json::to_value(value).unwrap()["markdown"][key],
                "#aabbcc"
            );
            let mut invalid = appearance(json!({ "version": 1, "markdown": { key: "url(x)" } }));
            assert!(invalid.validate().is_err());
        }
        for (key, max) in [
            ("paragraph_spacing", 96),
            ("heading_spacing", 96),
            ("list_indent", 96),
            ("list_spacing", 96),
            ("code_padding", 96),
            ("quote_padding", 96),
            ("table_cell_padding", 96),
            ("quote_border_width", 8),
            ("rule_width", 8),
            ("image_radius", 32),
            ("image_spacing", 96),
            ("code_border_width", 8),
            ("code_radius", 32),
            ("inline_code_padding", 96),
            ("inline_code_radius", 32),
            ("inline_code_border_width", 8),
            ("h1_spacing", 96),
            ("h2_spacing", 96),
            ("h3_spacing", 96),
            ("h4_spacing", 96),
            ("h5_spacing", 96),
            ("h6_spacing", 96),
        ] {
            for valid in [0, max] {
                appearance(json!({ "version": 1, "markdown": { key: valid } }))
                    .validate()
                    .unwrap();
            }
            assert!(
                appearance(json!({ "version": 1, "markdown": { key: max + 1 } }))
                    .validate()
                    .is_err()
            );
        }
        assert!(
            serde_json::from_value::<EmbedAppearance>(
                json!({ "version": 1, "markdown": { "unknown": 1 } })
            )
            .is_err()
        );
    }

    #[test]
    fn markdown_sizes_weights_and_choices_match_wire_contract() {
        let mut ranges = vec![
            ("image_width".to_owned(), 1, 100),
            ("code_size".to_owned(), 8, 48),
            ("inline_code_size".to_owned(), 8, 48),
        ];
        for level in 1..=6 {
            ranges.push((format!("h{level}_size"), 12, 96));
            ranges.push((format!("h{level}_weight"), 100, 900));
        }
        for (key, min, max) in ranges {
            for valid in [min, max] {
                appearance(json!({ "version": 1, "markdown": { &key: valid } }))
                    .validate()
                    .unwrap();
            }
            for invalid in [min - 1, max + 1] {
                assert!(
                    appearance(json!({ "version": 1, "markdown": { &key: invalid } }))
                        .validate()
                        .is_err()
                );
            }
            assert!(
                serde_json::from_value::<EmbedAppearance>(
                    json!({ "version": 1, "markdown": { &key: "24" } })
                )
                .is_err()
            );
        }
        for (key, choices) in [
            ("image_align", vec!["left", "center", "right"]),
            ("link_decoration", vec!["underline", "none"]),
            ("link_hover_decoration", vec!["underline", "none"]),
        ] {
            for choice in choices {
                appearance(json!({ "version": 1, "markdown": { key: choice } }))
                    .validate()
                    .unwrap();
            }
            assert!(
                serde_json::from_value::<EmbedAppearance>(
                    json!({ "version": 1, "markdown": { key: "invalid" } })
                )
                .is_err()
            );
        }
    }

    #[test]
    fn keyword_defaults_strip_to_an_empty_document_and_rgba_normalizes() {
        let mut value = json!({
            "version": 1,
            "colors": { "text": "Inherit", "background": "transparent", "link": "revert", "muted": "currentcolor" },
            "typography": { "body_size": 100, "heading_scale": "revert", "line_height": 160 },
            "layout": { "max_width": "none", "padding": 0, "spacing": "Revert" },
            "markdown": { "h1_size": "revert", "h1_weight": 700, "code_background": "transparent",
                "code_color": "inherit", "image_align": "center", "link_decoration": "underline" }
        });
        strip_default_sentinels(&mut value);
        assert_eq!(
            value,
            json!({
                "version": 1,
                "colors": { "background": "transparent" },
                "typography": { "body_size": 100, "line_height": 160 },
                "layout": { "padding": 0 },
                "markdown": { "h1_weight": 700, "image_align": "center", "link_decoration": "underline" }
            })
        );
        // Emptied groups stay; every group empty means "the default".
        assert!(!is_default_document(&value));
        assert!(!is_default_document(
            &json!({ "version": 1, "colors": { "background": "transparent" } })
        ));
        assert!(!is_default_document(&json!({
            "version": 1,
            "typography": { "body_size": 100 }
        })));
        let mut value = json!({
            "version": 1,
            "colors": { "text": "inherit" },
            "markdown": { "h1_size": "revert" }
        });
        strip_default_sentinels(&mut value);
        assert!(is_default_document(&value));
        assert!(is_default_document(&json!({ "version": 1 })));
        assert!(!is_default_document(&json!({ "version": 2 })));

        let mut value =
            appearance(json!({ "version": 1, "colors": { "border": "rgba(0, 0, 0, .15)" } }));
        value.validate().unwrap();
        assert_eq!(
            serde_json::to_value(value).unwrap()["colors"]["border"],
            "#00000026"
        );
        for (valid, stored) in [
            ("rgb(1, 2, 3)", "#010203ff"),
            ("rgba(255,0,0,0)", "#ff000000"),
            ("#aBc", "#aabbcc"),
            ("#abcdEF01", "#abcdef01"),
        ] {
            let mut value = appearance(json!({ "version": 1, "colors": { "text": valid } }));
            value.validate().unwrap();
            assert_eq!(
                serde_json::to_value(value).unwrap()["colors"]["text"],
                stored
            );
        }
        for invalid in [
            "rgb(1,2)",
            "rgba(1,2,3,1.5)",
            "rgb(a,b,c)",
            "#12345",
            "rgb(",
        ] {
            assert!(
                appearance(json!({ "version": 1, "colors": { "text": invalid } }))
                    .validate()
                    .is_err()
            );
        }
    }

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
            "typography": { "body_size": 110, "line_height": 180 },
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
        assert_eq!(value.typography.as_ref().unwrap().body_size, Some(110));
    }

    #[test]
    fn retired_fonts_are_ignored_in_storage_but_rejected_on_writes() {
        for field in ["font", "google_font"] {
            let raw = json!({
                "version": 1,
                "typography": { field: "serif", "body_size": 110 },
                "colors": { "text": "#abc" }
            });
            assert!(serde_json::from_value::<EmbedAppearance>(raw.clone()).is_err());
            let saved = parse_saved(&raw.to_string()).expect("legacy appearance remains usable");
            assert_eq!(saved.typography.as_ref().unwrap().body_size, Some(110));
            assert_eq!(
                saved.colors.as_ref().unwrap().text.as_deref(),
                Some("#aabbcc")
            );
            assert!(
                serde_json::to_value(saved).unwrap()["typography"]
                    .get(field)
                    .is_none()
            );
        }
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
