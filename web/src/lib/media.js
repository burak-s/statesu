import { Marked } from 'marked';

/** Remove matching Markdown images without rewriting prose or code examples. */
export function removeMediaImages(markdown, urls) {
	const targets = new Set(urls.filter(Boolean));
	const ranges = [];
	const marked = new Marked();
	function visit(tokens, source, base) {
		let cursor = 0;
		for (const token of tokens ?? []) {
			const start = source.indexOf(token.raw, cursor);
			if (start < 0) continue;
			cursor = start + token.raw.length;
			if (token.type === 'image' && targets.has(token.href)) {
				const suffix = /^\{width=\d{1,4}\}/.exec(source.slice(cursor));
				ranges.push([base + start, base + cursor + (suffix?.[0].length ?? 0)]);
			} else if (token.type !== 'code' && token.type !== 'codespan') {
				visit(token.tokens ?? token.items, token.raw, base + start);
			}
		}
	}
	visit(marked.lexer(markdown), markdown, 0);
	for (const [start, end] of ranges.sort((a, b) => b[0] - a[0])) {
		markdown = markdown.slice(0, start) + markdown.slice(end);
	}
	return markdown;
}
