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

// Shared by the parser and dashboard. Dimensions are pixels unless a unit is specified.
export const MARKDOWN_FIELDS = [
	['paragraph_spacing', 'Paragraph spacing', 'Text', 96],
	['heading_spacing', 'Heading spacing', 'Text', 96],
	['list_indent', 'List indentation', 'Lists', 96],
	['list_spacing', 'List item spacing', 'Lists', 96],
	['code_color', 'Text color', 'Code blocks'],
	['code_background', 'Background', 'Code blocks'],
	['code_padding', 'Padding', 'Code blocks', 96],
	['inline_code_color', 'Text color', 'Inline code'],
	['inline_code_background', 'Background', 'Inline code'],
	['quote_color', 'Text color', 'Blockquotes'],
	['quote_background', 'Background', 'Blockquotes'],
	['quote_border_color', 'Border color', 'Blockquotes'],
	['quote_border_width', 'Border width', 'Blockquotes', 8],
	['quote_padding', 'Padding', 'Blockquotes', 96],
	['table_header_background', 'Header background', 'Tables'],
	['table_border_color', 'Border color', 'Tables'],
	['table_cell_padding', 'Cell padding', 'Tables', 96],
	['image_radius', 'Corner radius', 'Images', 32],
	['rule_color', 'Divider color', 'Dividers'],
	['rule_width', 'Divider thickness', 'Dividers', 8],
	['link_decoration', 'Underline', 'Links', ['underline', 'none']],
	['link_hover_decoration', 'Hover underline', 'Links', ['underline', 'none']],
	['link_hover_color', 'Hover color', 'Links'],
	['code_size', 'Font size', 'Code blocks', 48, 8],
	['code_border_color', 'Border color', 'Code blocks'],
	['code_border_width', 'Border width', 'Code blocks', 8],
	['code_radius', 'Corner radius', 'Code blocks', 32],
	['inline_code_size', 'Font size', 'Inline code', 48, 8],
	['inline_code_padding', 'Padding', 'Inline code', 96],
	['inline_code_radius', 'Corner radius', 'Inline code', 32],
	['inline_code_border_color', 'Border color', 'Inline code'],
	['inline_code_border_width', 'Border width', 'Inline code', 8],
	['table_header_color', 'Header text color', 'Tables'],
	['table_stripe_background', 'Alternating row background', 'Tables'],
	['image_width', 'Width', 'Images', 100, 1, '%'],
	['image_align', 'Alignment', 'Images', ['left', 'center', 'right']],
	['image_spacing', 'Vertical spacing', 'Images', 96],
	...[1, 2, 3, 4, 5, 6].flatMap((level) => [
		[`h${level}_size`, 'Font size', `H${level}`, 96, 12],
		[`h${level}_weight`, 'Font weight', `H${level}`, 900, 100, ''],
		[`h${level}_spacing`, 'Vertical spacing', `H${level}`, 96]
	])
].map(([key, label, section, max, min = 0, unit = 'px']) => ({
	key, label, section, color: max === undefined,
	options: Array.isArray(max) ? max : undefined,
	max: Array.isArray(max) ? undefined : max, min, unit
}));
for (const field of MARKDOWN_FIELDS) {
	if (!field.color && !field.options) RANGES[field.key] = [field.min, field.max];
}

/// UA heading font sizes; the scale token multiplies them so an unset scale
/// reproduces the browser default exactly.
const HEADING_SIZES = { h1: '2em', h2: '1.5em', h3: '1.17em', h4: '1em', h5: '.83em', h6: '.67em' };

const HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;

// Keyword defaults: the spelling of "keep the built-in or website-inherited
// look". stripDefaults removes them before saving, mirroring the server.
const COLOR_SENTINELS = new Set(['inherit', 'revert', 'currentcolor', 'transparent']);

function hexFromChannels(channels) {
	return `#${channels.map((channel) => channel.toString(16).padStart(2, '0')).join('')}`;
}

function colorChannel(part) {
	const value = Number(part);
	return Number.isInteger(value) && value >= 0 && value <= 255 ? value : null;
}

/// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()` and `rgba()`, all
/// normalized to lowercase `#rrggbb[aa]`. Keyword defaults return undefined
/// so they drop out of normalized documents.
function cssColor(value) {
	if (typeof value !== 'string') return null;
	const text = value.trim().toLowerCase();
	if (COLOR_SENTINELS.has(text)) return undefined;
	if (text.startsWith('#')) {
		if (!HEX.test(text)) return null;
		const hex = text.slice(1);
		const expanded = hex.length <= 4
			? hex.split('').map((ch) => ch + ch).join('')
			: hex;
		return `#${expanded}`;
	}
	const functionMatch = text.match(/^rgba?\(([^)]+)\)$/);
	if (!functionMatch) return null;
	const parts = functionMatch[1].split(',');
	if (parts.length !== 3 && parts.length !== 4) return null;
	const channels = [];
	for (const part of parts.slice(0, 3)) {
		const channel = colorChannel(part.trim());
		if (channel === null) return null;
		channels.push(channel);
	}
	if (parts.length === 4) {
		const alpha = Number(parts[3].trim());
		if (!Number.isFinite(alpha) || alpha < 0 || alpha > 1) return null;
		channels.push(Math.round(alpha * 255));
	} else {
		channels.push(255);
	}
	return hexFromChannels(channels);
}

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
			body_size: percent(RANGES.body_size),
			heading_scale: percent(RANGES.heading_scale),
			line_height: percent(RANGES.line_height)
		}, ['font', 'google_font']),
		layout: group(value.layout, {
			max_width: (raw) => raw === 'none' ? undefined : pixel(RANGES.max_width)(raw),
			padding: pixel(RANGES.padding),
			spacing: pixel(RANGES.spacing)
		}),
		markdown: group(value.markdown, Object.fromEntries(MARKDOWN_FIELDS.map((field) =>
			[field.key, field.color ? color : field.options
				? (value) => field.options.includes(value) ? value : null
				: pixel(RANGES[field.key])]
		))),
		decoration: group(value.decoration, {
			border_width: pixel(RANGES.border_width),
			corner_radius: pixel(RANGES.corner_radius)
		})
	};
}

export const APPEARANCE_FIELDS = {
	colors: ['text', 'background', 'link', 'muted', 'border'],
	typography: ['body_size', 'heading_scale', 'line_height'],
	layout: ['max_width', 'padding', 'spacing'],
	decoration: ['border_width', 'corner_radius'],
	markdown: MARKDOWN_FIELDS.map((field) => field.key)
};

// The stock look, written out field by field: keywords name defaults that
// resolve to inherited values (text color, spacing), numbers are fixed px or
// percentages. Saving a document of only defaults is equivalent to clearing.
const APPEARANCE_DEFAULTS = {
	colors: {
		text: 'inherit', background: 'transparent', link: 'revert',
		muted: 'currentcolor', border: 'rgba(0,0,0,.15)'
	},
	typography: { body_size: 100, heading_scale: 100, line_height: 160 },
	layout: { max_width: 'none', padding: 0, spacing: 'revert' },
	decoration: { border_width: 0, corner_radius: 0 },
	markdown: {
		paragraph_spacing: 'revert', heading_spacing: 'revert',
		list_indent: 40, list_spacing: 0,
		code_color: 'inherit', code_background: 'transparent', code_padding: 0,
		code_size: 'revert', code_border_color: 'currentcolor', code_border_width: 0, code_radius: 0,
		inline_code_color: 'inherit', inline_code_background: 'transparent',
		inline_code_size: 'revert', inline_code_padding: 0, inline_code_radius: 0,
		inline_code_border_color: 'currentcolor', inline_code_border_width: 0,
		quote_color: 'inherit', quote_background: 'transparent',
		quote_border_color: 'currentcolor', quote_border_width: 0, quote_padding: 0,
		table_header_background: 'transparent', table_header_color: 'inherit',
		table_border_color: 'transparent', table_cell_padding: 1, table_stripe_background: 'transparent',
		link_decoration: 'underline', link_hover_decoration: 'underline', link_hover_color: 'revert',
		image_radius: 0, image_width: 'revert', image_align: 'center', image_spacing: 'revert',
		rule_color: 'currentcolor', rule_width: 1,
		...Object.fromEntries([1, 2, 3, 4, 5, 6].flatMap((level) => [
			[`h${level}_size`, 'revert'], [`h${level}_weight`, 700], [`h${level}_spacing`, 'revert']
		]))
	}
};

export function appearanceTemplate(value) {
	return {
		version: APPEARANCE_VERSION,
		...Object.fromEntries(Object.entries(APPEARANCE_FIELDS).map(([group, keys]) => [
			group, Object.fromEntries(keys.map((key) => [key, value?.[group]?.[key] ?? APPEARANCE_DEFAULTS[group][key]]))
		]))
	};
}

/// Removes keyword defaults (and normalizes color spellings) so drafts sent
/// to the API carry only real values; the server applies the same rule.
export function stripDefaults(document) {
	if (!document || typeof document !== 'object' || Array.isArray(document)) return document;
	const out = { ...document };
	for (const [group, keys] of Object.entries(APPEARANCE_FIELDS)) {
		const fields = out[group];
		if (!fields || typeof fields !== 'object' || Array.isArray(fields)) continue;
		const next = { ...fields };
		for (const key of Object.keys(next)) {
				const raw = next[key];
			if (raw === undefined) { delete next[key]; continue; }
			if (typeof raw !== 'string') continue;
			const text = raw.trim().toLowerCase();
			const field = group === 'markdown' ? MARKDOWN_FIELDS.find((candidate) => candidate.key === key) : undefined;
			if (field?.options) continue;
			const isColor = group === 'colors' || field?.color;
			if (isColor) {
				const normalized = cssColor(raw);
				// The global background keeps an explicit `transparent`: it is
				// stored and delivered as a real value, unlike the sentinels.
				if (normalized === undefined && !(group === 'colors' && key === 'background')) delete next[key];
				else if (normalized !== null && normalized !== undefined) next[key] = normalized;
			} else if (text === 'revert' || (text === 'none' && key === 'max_width')) {
				delete next[key];
			}
		}
		out[group] = next;
		if (Object.keys(next).length === 0) delete out[group];
	}
	return out;
}

// Strict authoring validation for JSON mode. Delivery parsing remains forgiving
// for old embeds, but edits must never silently drop unknown or malformed fields.
export function validateAppearanceDocument(value) {
	if (value === null) return '';
	if (!value || typeof value !== 'object' || Array.isArray(value)) return 'Expected an appearance object or null.';
	if (value.version !== APPEARANCE_VERSION) return 'version must be 1.';
	const groups = Object.keys(APPEARANCE_FIELDS);
	for (const key of Object.keys(value)) {
		if (key !== 'version' && !groups.includes(key)) return `Unknown field: ${key}. Use the appearance document without an embed_appearance wrapper.`;
	}
	for (const name of groups) {
		const fields = value[name];
		if (fields === undefined || fields === null) continue;
		if (typeof fields !== 'object' || Array.isArray(fields)) return `${name} must be an object.`;
		for (const [key, entry] of Object.entries(fields)) {
			if (!APPEARANCE_FIELDS[name].includes(key)) return `Unknown field: ${name}.${key}.`;
			const parsed = parseAppearance({ version: 1, [name]: { [key]: entry === null ? undefined : entry } });
			if (parsed[name] === null) {
				const field = name === 'markdown' ? MARKDOWN_FIELDS.find((field) => field.key === key) : undefined;
				const expected = field?.options ? field.options.join(' or ')
					: RANGES[key] ? `an integer between ${RANGES[key][0]} and ${RANGES[key][1]}`
					: `a #rgb or #rrggbb color${name === 'colors' && key === 'background' ? ', or transparent' : ''}`;
				return `${name}.${key} must be ${expected}.`;
			}
		}
	}
	return '';
}

/// Applies one group with per-field validators; unknown or invalid fields
/// reject the whole group (same all-or-nothing stance as the backend).
/// Retired font fields are ignored for compatibility with older deliveries.
function group(value, fields, retired = []) {
	if (value === undefined) return undefined;
	if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
	const allowed = Object.keys(fields);
	for (const key of Object.keys(value)) {
		if (!allowed.includes(key) && !retired.includes(key)) return null;
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
	return cssColor(value);
}

/// Colors plus the literal `transparent`.
function background(value) {
	if (typeof value === 'string' && value.trim().toLowerCase() === 'transparent') return 'transparent';
	return color(value);
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
	if (value === 'revert') return undefined;
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
	if (appearance.markdown) {
		for (const field of MARKDOWN_FIELDS) {
			const value = appearance.markdown[field.key];
			if (value === undefined) continue;
			vars[`--statesu-internal-${field.key.replaceAll('_', '-')}`] = field.color || field.options ? value : `${value}${field.unit}`;
			if (field.key === 'image_align') {
				vars['--statesu-internal-image-margin-left'] = value === 'left' ? '0' : 'auto';
				vars['--statesu-internal-image-margin-right'] = value === 'right' ? '0' : 'auto';
			}
		}
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
function markdownStyles() {
	const token = (name, fallback) => `var(--statesu-${name}, var(--statesu-internal-${name}, ${fallback}))`;
	return `
.statesu-v2 .statesu-body p { margin-block: ${token('paragraph-spacing', 'var(--statesu-spacing, var(--statesu-internal-spacing, revert))')}; }
.statesu-v2 .statesu-body :is(h1,h2,h3,h4,h5,h6) { margin-block: ${token('heading-spacing', 'revert')}; }
.statesu-v2 .statesu-body :is(ul,ol) { padding-inline-start: ${token('list-indent', '40px')}; }
.statesu-v2 .statesu-body li { margin-block: ${token('list-spacing', '0')}; }
.statesu-v2 .statesu-body pre { color: ${token('code-color', 'inherit')}; background: ${token('code-background', 'transparent')}; padding: ${token('code-padding', '0')}; }
.statesu-v2 .statesu-body :not(pre) > code { color: ${token('inline-code-color', 'inherit')}; background: ${token('inline-code-background', 'transparent')}; }
.statesu-v2 .statesu-body blockquote { color: ${token('quote-color', 'inherit')}; background: ${token('quote-background', 'transparent')}; border-inline-start: ${token('quote-border-width', '0')} solid ${token('quote-border-color', 'currentColor')}; padding: ${token('quote-padding', '0')}; }
.statesu-v2 .statesu-body table { display: block; max-width: 100%; overflow-x: auto; border-collapse: collapse; }
.statesu-v2 .statesu-body :is(th,td) { padding: ${token('table-cell-padding', '1px')}; border: 1px solid ${token('table-border-color', 'transparent')}; }
.statesu-v2 .statesu-body th { background: ${token('table-header-background', 'transparent')}; }
.statesu-v2 .statesu-body img { border-radius: ${token('image-radius', '0')}; }
.statesu-v2 .statesu-body hr { border: 0; border-top: ${token('rule-width', '1px')} solid ${token('rule-color', 'currentColor')}; }
.statesu-v2 .statesu-body a { text-decoration: ${token('link-decoration', 'underline')}; }
.statesu-v2 .statesu-body a:any-link:is(:hover,:focus-visible) { color: ${token('link-hover-color', 'var(--statesu-link-color, var(--statesu-internal-link, revert))')}; text-decoration: ${token('link-hover-decoration', token('link-decoration', 'underline'))}; }
.statesu-v2 .statesu-body pre { font-size: ${token('code-size', 'revert')}; border: ${token('code-border-width', '0')} solid ${token('code-border-color', 'currentColor')}; border-radius: ${token('code-radius', '0')}; }
.statesu-v2 .statesu-body :not(pre) > code { font-size: ${token('inline-code-size', 'revert')}; padding: ${token('inline-code-padding', '0')}; border-radius: ${token('inline-code-radius', '0')}; border: ${token('inline-code-border-width', '0')} solid ${token('inline-code-border-color', 'currentColor')}; }
.statesu-v2 .statesu-body th { color: ${token('table-header-color', 'inherit')}; }
.statesu-v2 .statesu-body tbody tr:nth-child(even) { background: ${token('table-stripe-background', 'transparent')}; }
.statesu-v2 .statesu-body img { width: ${token('image-width', 'revert')}; margin-block: ${token('image-spacing', '1.25em')}; }
.statesu-v2 .statesu-body img:not(.linked-in) { margin-left: ${token('image-margin-left', 'auto')}; margin-right: ${token('image-margin-right', 'auto')}; }
${Object.entries(HEADING_SIZES).map(([tag, size]) => `.statesu-v2 .statesu-body ${tag} {
 font-size: var(--statesu-heading-size, ${token(`${tag}-size`, `calc(${size} * var(--statesu-heading-scale, var(--statesu-internal-heading-scale, 1)))`)});
 font-weight: ${token(`${tag}-weight`, 'bold')}; margin-block: ${token(`${tag}-spacing`, token('heading-spacing', 'revert'))};
}`).join('\n')}
`;
}

export function embedStyles() {
	return `
.statesu-v2 {
	color: var(--statesu-text-color, var(--statesu-internal-text, inherit));
	background-color: var(--statesu-background-color, var(--statesu-internal-background, transparent));
	font-family: var(--statesu-font-family, inherit);
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
/* Revert to the browser's own margins when unset, including the zero
   margins on nested lists. Configured spacing applies to every block. */
.statesu-v2 .statesu-body p,
.statesu-v2 .statesu-body ul,
.statesu-v2 .statesu-body ol,
.statesu-v2 .statesu-body blockquote,
.statesu-v2 .statesu-body pre {
	margin-block: var(--statesu-spacing, var(--statesu-internal-spacing, revert));
}
/* Both link states follow configured colors. Without a token, revert
   preserves native unvisited/visited colors instead of forcing one color. */
.statesu-v2 .statesu-body a:any-link {
	color: var(--statesu-link-color, var(--statesu-internal-link, revert));
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
${markdownStyles()}
@media (prefers-reduced-motion: reduce) {
	.statesu-v2 .statesu-body img { transition: none; }
}`;
}
