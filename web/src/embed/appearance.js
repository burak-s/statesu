// Saved embed appearance for a blog: parsing, validation and application.
//
// The public delivery API ships the blog's saved appearance (typed style
// tokens, never CSS text) next to its profile. This module validates the
// payload defensively — mirrors of the server-side bounds — and turns the
// tokens into internal custom properties on the embed root.
//
// Variable layering, from strongest to weakest:
//   1. Public overrides set by the host page (`--statesu-*`, documented).
//   2. Internal tokens from the saved appearance (`--statesu-internal-*`,
//      set inline on the root by this module).
//   3. The built-in defaults written into the stylesheet.
// Public variables sit first in every `var()` chain, so a host page can
// always win without touching the shadow tree.

export const APPEARANCE_VERSION = 1;

/**
 * Bounds mirror src/blog/appearance.rs; sizes are percentages (100 = the
 * built-in value), layout and decoration are pixels. Shared with the
 * dashboard editor so both validate against one contract.
 * @type {{ [key: string]: [number, number] }}
 */
export const RANGES = {
	body_size: [75, 200],
	heading_scale: [100, 225],
	line_height: [100, 250],
	max_width: [320, 1200],
	padding: [0, 96],
	spacing: [0, 96],
	border_width: [0, 8],
	corner_radius: [0, 32]
};

const FONT_STACKS = {
	sans: 'ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif',
	serif: 'ui-serif, Georgia, Cambria, "Times New Roman", serif',
	mono: 'ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace'
};

/// UA heading font sizes; the scale token multiplies them so an unset scale
/// reproduces the browser default exactly.
const HEADING_SIZES = { h1: '2em', h2: '1.5em', h3: '1.17em', h4: '1em', h5: '.83em', h6: '.67em' };

const HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i;

/**
 * Validates the delivered appearance document. Returns a normalized shape
 * with camelCase keys, or null when anything is off — an unparsable,
 * unsupported or out-of-range payload must never break content delivery.
 */
export function parseAppearance(value) {
	if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
	if (value.version !== APPEARANCE_VERSION) return null;
	return {
		colors: group(value.colors, {
			text: color,
			background: background,
			link: color,
			muted: color,
			border: color
		}),
		typography: group(value.typography, {
			font: font,
			body_size: percent(RANGES.body_size),
			heading_scale: percent(RANGES.heading_scale),
			line_height: percent(RANGES.line_height)
		}),
		layout: group(value.layout, {
			max_width: pixel(RANGES.max_width),
			padding: pixel(RANGES.padding),
			spacing: pixel(RANGES.spacing)
		}),
		decoration: group(value.decoration, {
			border_width: pixel(RANGES.border_width),
			corner_radius: pixel(RANGES.corner_radius)
		})
	};
}

/// Applies one group with per-field validators; unknown or invalid fields
/// reject the whole group (same all-or-nothing stance as the backend).
function group(value, fields) {
	if (value === undefined) return undefined;
	if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
	const allowed = Object.keys(fields);
	for (const key of Object.keys(value)) {
		if (!allowed.includes(key)) return null;
	}
	const out = {};
	for (const [key, validate] of Object.entries(fields)) {
		if (value[key] === undefined) continue;
		const normalized = validate(value[key]);
		if (normalized === null) return null;
		out[key] = normalized;
	}
	return out;
}

/// `#rgb` and `#rrggbb`, normalized to lowercase `#rrggbb`.
function color(value) {
	if (typeof value !== 'string' || !HEX.test(value)) return null;
	const hex = value.slice(1);
	const expanded = hex.length === 3 ? hex.split('').map((ch) => ch + ch).join('') : hex;
	return `#${expanded.toLowerCase()}`;
}

/// Colors plus the literal `transparent`.
function background(value) {
	if (value === 'transparent') return 'transparent';
	if (typeof value === 'string' && value.toLowerCase() === 'transparent') return 'transparent';
	return color(value);
}

function font(value) {
	// `inherit` is a valid backend choice meaning "the host page's font".
	return value === 'inherit' || FONT_STACKS[value] ? value : null;
}

/// Integer percentage within bounds; booleans and floats are rejected.
function percent([min, max]) {
	return (value) => integer(value, min, max);
}

/// Integer pixel count within bounds.
function pixel([min, max]) {
	return (value) => integer(value, min, max);
}

function integer(value, min, max) {
	if (typeof value !== 'number' || !Number.isInteger(value)) return null;
	return value >= min && value <= max ? value : null;
}

/**
 * Maps a normalized appearance to the internal custom properties the
 * stylesheet consumes. Unset tokens are simply absent, so the built-in
 * defaults keep ruling them.
 */
export function appearanceVars(appearance) {
	if (!appearance) return {};
	const vars = {};
	const { colors, typography, layout, decoration } = appearance;
	if (colors) {
		vars['--statesu-internal-text'] = colors.text;
		vars['--statesu-internal-background'] = colors.background;
		vars['--statesu-internal-link'] = colors.link;
		vars['--statesu-internal-muted'] = colors.muted;
		vars['--statesu-internal-border-color'] = colors.border;
	}
	if (typography) {
		// `inherit` means "the host page's font": no declaration at all.
		vars['--statesu-internal-font'] = typography.font && typography.font !== 'inherit'
			? FONT_STACKS[typography.font]
			: undefined;
		vars['--statesu-internal-body-size'] = typography.body_size !== undefined
			? rem(typography.body_size)
			: undefined;
		// A unitless multiplier: each heading rule applies it to its own
		// built-in size, so unset stays at the browser default.
		vars['--statesu-internal-heading-scale'] = typography.heading_scale !== undefined
			? String(typography.heading_scale / 100)
			: undefined;
		vars['--statesu-internal-line-height'] = typography.line_height !== undefined
			? String(typography.line_height / 100)
			: undefined;
	}
	if (layout) {
		vars['--statesu-internal-max-width'] = px(layout.max_width);
		vars['--statesu-internal-padding'] = px(layout.padding);
		vars['--statesu-internal-spacing'] = px(layout.spacing);
	}
	if (decoration) {
		vars['--statesu-internal-border-width'] = px(decoration.border_width);
		vars['--statesu-internal-corner-radius'] = px(decoration.corner_radius);
	}
	for (const key of Object.keys(vars)) {
		if (vars[key] === undefined) delete vars[key];
	}
	return vars;
}

/// Percent sizes stay relative (to the host page's root font size) and read
/// as rem so the embed scales with the page it lives in.
function rem(percentValue) {
	return `${percentValue / 100}rem`;
}

function px(value) {
	return value === undefined ? undefined : `${value}px`;
}

/**
 * Writes the saved tokens onto the embed root. Runs whenever the appearance
 * payload lands, before or after content rendering: custom properties
 * inherit, so already-rendered children pick them up too.
 */
export function applyAppearance(root, appearance) {
	for (const [name, value] of Object.entries(appearanceVars(appearance))) {
		root.style.setProperty(name, value);
	}
}

/**
 * The shadow stylesheet for embed v2. Every tokenized rule reads
 * `var(--statesu-<token>, var(--statesu-internal-<token>, <built-in>))`:
 * host-page overrides first, saved appearance second, stock look last.
 * Also shared with the dashboard's appearance preview, which must style
 * posts exactly like production embeds.
 */
export function embedStyles() {
	return `
.statesu-v2 {
	color: var(--statesu-text-color, var(--statesu-internal-text, inherit));
	background-color: var(--statesu-background-color, var(--statesu-internal-background, transparent));
	font-family: var(--statesu-font-family, var(--statesu-internal-font, inherit));
	font-size: var(--statesu-body-size, var(--statesu-internal-body-size, 1rem));
	line-height: var(--statesu-line-height, var(--statesu-internal-line-height, 1.6));
	max-width: var(--statesu-max-width, var(--statesu-internal-max-width, none));
	box-sizing: border-box;
	padding: var(--statesu-padding, var(--statesu-internal-padding, 0));
	border-width: var(--statesu-border-width, var(--statesu-internal-border-width, 0));
	border-style: solid;
	border-color: var(--statesu-border-color, var(--statesu-internal-border-color, rgba(0, 0, 0, .15)));
	border-radius: var(--statesu-corner-radius, var(--statesu-internal-corner-radius, 0));
}
.statesu-v2 .statesu-state {
	color: var(--statesu-muted-color, var(--statesu-internal-muted, currentColor));
	opacity: .75;
	font-style: italic;
}
.statesu-v2 .statesu-error {
	color: var(--statesu-muted-color, var(--statesu-internal-muted, currentColor));
	opacity: .85;
}
.statesu-v2 .statesu-title {
	margin: 0 0 var(--statesu-spacing, var(--statesu-internal-spacing, .5em));
	font-size: var(--statesu-heading-size, calc(1.5em * var(--statesu-heading-scale, var(--statesu-internal-heading-scale, 1))));
}
${Object.entries(HEADING_SIZES).map(([tag, size]) =>
	`.statesu-v2 .statesu-body ${tag} { font-size: var(--statesu-heading-size, calc(${size} * var(--statesu-heading-scale, var(--statesu-internal-heading-scale, 1)))); }`
).join('\n')}
.statesu-v2 .statesu-body img { max-width: 100%; height: auto; display: block; margin: 1.25em auto; }
.statesu-v2 .statesu-body img.linked-in { display: inline; margin: 0; }
.statesu-v2 .statesu-body pre { overflow-x: auto; }
/* Content rhythm: paragraph, list, quote and code spacing follow the
   spacing token; 1em is the built-in UA block margin, so unset stays stock.
   Links keep UA behavior when no token exists — only unvisited links are
   tokenized, so visited styling is never overridden. */
.statesu-v2 .statesu-body p,
.statesu-v2 .statesu-body ul,
.statesu-v2 .statesu-body ol,
.statesu-v2 .statesu-body blockquote,
.statesu-v2 .statesu-body pre {
	margin-block: var(--statesu-spacing, var(--statesu-internal-spacing, 1em));
}
.statesu-v2 .statesu-body a:link {
	color: var(--statesu-link-color, var(--statesu-internal-link, LinkText));
	text-decoration: underline;
}
.statesu-v2 .statesu-body a { text-decoration: underline; }
.statesu-v2 .statesu-announcement + .statesu-announcement {
	margin-top: var(--statesu-spacing, var(--statesu-internal-spacing, 2rem));
	padding-top: var(--statesu-spacing, var(--statesu-internal-spacing, 2rem));
	border-top: 1px solid var(--statesu-border-color, var(--statesu-internal-border-color, currentColor));
}
.statesu-v2 .statesu-more {
	margin-top: 1.5rem;
	font: inherit;
	color: inherit;
	background: none;
	border: 0;
	padding: 0;
	text-decoration: underline;
	cursor: pointer;
}
@media (prefers-reduced-motion: reduce) {
	.statesu-v2 .statesu-body img { transition: none; }
}`;
}
