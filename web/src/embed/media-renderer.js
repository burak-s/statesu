// Media-capable Markdown rendering, shared by the embed v2 and the
// dashboard preview:
//   marked (isolated instance; raw HTML dropped; images + {width} allowed)
//   → DOMPurify (extended allowlist, strict URL policy for links and imgs)
// Everything is bundled; there is no runtime CDN dependency and no escape
// hatch back to raw HTML. Metadata (responsive variants) maps canonical
// source URLs to generated sizes so only trusted Statesu media URLs can
// grow a srcset; unknown external images render as plain <img>.
//
// Standard syntax stays standard:
//   ![Alt](https://media.example/statesu/blogs/…/source.webp)
// The one documented extension is a bounded width pin:
//   ![Alt](…){width=640}

import { Marked } from 'marked';
import createDOMPurify from 'dompurify';
import { safeTarget } from './content.js';

const SAFE_PROTOCOLS = new Set(['http:', 'https:']);

const ALLOWED_TAGS = [
	'p', 'br', 'hr',
	'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
	'ul', 'ol', 'li',
	'blockquote', 'pre', 'code',
	'em', 'strong', 'del', 's', 'sup', 'sub',
	'table', 'thead', 'tbody', 'tr', 'th', 'td',
	'span', 'a', 'img'
];

const ALLOWED_ATTR = [
	'href', 'title',
	'src', 'alt', 'width', 'srcset', 'sizes', 'loading'
];

// 1..9999 rendered pixels; anything beyond pins can stay unpinned and
// shrink to its container anyway.
const WIDTH_PATTERN = /^!\[([^\]\n]*)\]\(\s*<?([^\s>]+)>?(?:\s+(?:"([^"]*)"|\([^)]*\)))?\s*\)\{width=(\d{1,4})\}/;

/** @param {number} value */
function clampWidth(value) {
	return Number.isInteger(value) && value >= 1 && value <= 9999 ? value : null;
}

/** @param {any} window @param {any} href */
function safeUrl(window, href) {
	if (typeof href !== 'string' || href === '') return null;
	try {
		const url = new URL(href, window.location.href);
		return SAFE_PROTOCOLS.has(url.protocol) ? url.href : null;
	} catch {
		return null;
	}
}

/** @param {any} value */
function escapeAttr(value) {
	return String(value ?? '')
		.replaceAll('&', '&amp;')
		.replaceAll('"', '&quot;')
		.replaceAll('<', '&lt;')
		.replaceAll('>', '&gt;');
}

/** Builds one <img> tag; metadata only ever adds trusted sizes. */
/** @param {any} window @param {Map<string, any>} mediaByUrl @param {any} token */
export function renderImageMarkup(window, mediaByUrl, token) {
	const alt = escapeAttr(token.text ?? token.alt ?? '');
	const title = token.title ? ` title="${escapeAttr(token.title)}"` : '';
	const src = safeUrl(window, token.href ?? token.src ?? '');
	// Without a safe URL the words stay, the image does not.
	if (!src) return escapeAttr(token.text ?? token.alt ?? '');
	const meta = mediaByUrl.get(src) ?? null;
	const width = clampWidth(token.width) ?? clampWidth(meta?.source_width);
	const parts = [`src="${escapeAttr(src)}"`, `alt="${alt}"`];
	if (title !== '') parts.push(title.trim());
	if (width !== null) parts.push(`width="${width}"`);
	if (meta) {
		const srcset = (meta.variants ?? [])
			.map((variant) => `${escapeAttr(variant.url)} ${variant.width}w`)
			.join(', ');
		if (srcset !== '') {
			parts.push(`srcset="${srcset}"`);
			parts.push(`sizes="${width === null ? '100vw' : `(max-width: 640px) 100vw, min(${width}px, 100vw)`}"`);
		}
	}
	return `<img ${parts.join(' ')} loading="lazy">`;
}

/**
 * Builds the shared media renderer for one page. Returns null when
 * DOMPurify cannot run — callers must fall back to safe text.
 */
/** @param {any} window @param {Map<string, any>} mediaByUrl */
export function buildMediaRenderer(window, mediaByUrl = new Map()) {
	if (!window || typeof createDOMPurify !== 'function') return null;
	const domPurify = createDOMPurify(window);
	if (!domPurify || typeof domPurify.sanitize !== 'function' || !domPurify.isSupported) {
		return null;
	}

	// URL policy runs after sanitization so no payload-supplied attribute
	// decision is ever trusted.
	domPurify.addHook('afterSanitizeAttributes', (node) => {
		if (typeof node.getAttribute !== 'function') return;
		if (node.nodeName === 'A') {
			const href = node.getAttribute('href');
			const target = safeTarget(window, href);
			if (!target || !SAFE_PROTOCOLS.has(target.protocol)) {
				// Anchors without a safe href lose the link, not the words.
				const text = window.document.createTextNode(node.textContent ?? '');
				node.parentNode?.insertBefore(text, node);
				node.remove();
				return;
			}
			node.setAttribute('target', '_blank');
			node.setAttribute('rel', 'noopener noreferrer');
			return;
		}
		if (node.nodeName === 'IMG') {
			const src = node.getAttribute('src');
			const url = src === null ? null : safeUrl(window, src);
			if (!url) {
				node.remove();
				return;
			}
			// Force the lazy flag; the renderer's attribute carries intent,
			// the hook's decision is trusted output.
			node.setAttribute('loading', 'lazy');
		}
	});

	// markd instances are isolated so embed v1 on the same page keeps its
	// own, stricter rules untouched.
	const marked = new Marked({
		gfm: true,
		breaks: false,
		extensions: [
			{
				name: 'mediaImage',
				level: 'inline',
				// Width suffix tokens must run before the standard image
				// tokenizer, which would otherwise swallow the suffix.
				start(src) {
					const index = src.indexOf('![');
					return index === -1 ? undefined : index;
				},
				tokenizer(src) {
					const match = WIDTH_PATTERN.exec(src);
					if (!match) return undefined;
					return {
						type: 'mediaImage',
						raw: match[0],
						text: match[1],
						href: match[2],
						title: match[3] ?? '',
						width: Number.parseInt(match[4], 10),
						tokens: []
					};
				},
				renderer(token) {
					return renderImageMarkup(window, mediaByUrl, token);
				}
			}
		],
		renderer: {
			// Raw HTML never reaches the DOM, no escape hatch.
			html: () => '',
			image(token) {
				return renderImageMarkup(window, mediaByUrl, token);
			}
		}
	});

	return (/** @type {any} */ markdown) =>
		String(
			domPurify.sanitize(String(marked.parse(String(markdown))), {
				ALLOWED_TAGS,
				ALLOWED_ATTR,
				ALLOW_DATA_ATTR: false,
				RETURN_DOM: false
			})
		);
}
