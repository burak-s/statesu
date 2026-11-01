// Markdown rendering pipeline for the embed, hardened by default:
//   marked (raw HTML and images dropped at the renderer layer)
//   → DOMPurify (strict allowlist)
//   → link policy (http(s) only, forced target/rel, text preserved)
// Everything is bundled; there is no runtime CDN dependency and no escape
// hatch back to raw HTML. If no sanitizer is available the pipeline reports
// itself unavailable and the caller must fall back to safe text.

import { marked } from 'marked';
import createDOMPurify from 'dompurify';

// Drop raw HTML tokens and images before any HTML exists. No regex-based
// cleanup anywhere: the parser layer removes, the sanitizer decides.
marked.use({
	gfm: true,
	breaks: false,
	renderer: {
		html: () => '',
		image: () => ''
	}
});

const ALLOWED_TAGS = [
	'p', 'br', 'hr',
	'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
	'ul', 'ol', 'li',
	'blockquote', 'pre', 'code',
	'em', 'strong', 'del', 's', 'sup', 'sub',
	'table', 'thead', 'tbody', 'tr', 'th', 'td',
	'span', 'a'
];

const SAFE_PROTOCOLS = new Set(['http:', 'https:']);

export function safeTarget(window, href) {
	if (href === null || href === '') return null;
	try {
		return new URL(href, window.location.href);
	} catch {
		return null;
	}
}

/** Replaces an anchor with its plain text: link gone, words stay. */
function demoteToText(window, node) {
	const text = window.document.createTextNode(node.textContent ?? '');
	const parent = node.parentNode;
	if (parent) {
		parent.insertBefore(text, node);
	}
	node.remove();
}

/**
 * Builds the render function for one page. Returns null when DOMPurify
 * cannot run — callers must then treat all content as unrenderable HTML and
 * fall back to text.
 */
export function buildRenderer(window) {
	if (!window || typeof createDOMPurify !== 'function') return null;
	const domPurify = createDOMPurify(window);
	// `isSupported` is DOMPurify's own verdict: an environment that cannot
	// sanitize must never let raw markup through.
	if (!domPurify || typeof domPurify.sanitize !== 'function' || !domPurify.isSupported) {
		return null;
	}

	domPurify.addHook('afterSanitizeAttributes', (node) => {
		if (node.nodeName !== 'A' || typeof node.getAttribute !== 'function') return;
		const href = node.getAttribute('href');
		const target = safeTarget(window, href);
		if (!target || !SAFE_PROTOCOLS.has(target.protocol)) {
			// An anchor without a safe href is not a link: the words around it
			// stay, only the anchor element is removed.
			demoteToText(window, node);
			return;
		}
		// Trusted render stage produces the fixed attributes; payload-supplied
		// target/rel values are never honored.
		node.setAttribute('target', '_blank');
		node.setAttribute('rel', 'noopener noreferrer');
	});

	return (markdown) =>
		String(
			domPurify.sanitize(String(marked.parse(String(markdown))), {
				ALLOWED_TAGS,
				ALLOWED_ATTR: ['href', 'title'],
				ALLOW_DATA_ATTR: false,
				RETURN_DOM: false
			})
		);
}
