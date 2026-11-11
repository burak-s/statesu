import test from 'node:test';
import assert from 'node:assert/strict';
import { isValidPostPath, originFromSiteAddress } from '../src/lib/integration.js';
import { removeMediaImages } from '../src/lib/media.js';
import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';
import { buildMediaRenderer } from '../src/embed/media-renderer.js';

const dashboard = readFileSync(new URL('../src/routes/dashboard/+page.svelte', import.meta.url), 'utf8');

async function deleteDashboardMedia(request, confirmed = true) {
	const source = dashboard.match(/async function deleteMediaEntry\(mediaEntry\) \{[\s\S]*?\n\t\}/)[0];
	const run = new Function('request', 'removeMediaImages', 'confirmed', `
		let selectedBlogId = 'blog';
		let mediaError = '';
		let deletingMediaId = null;
		let uploading = false;
		let mediaNote = '';
		let editor = { content_markdown: 'Before ![photo](https://img.test/source.webp) ![small](https://img.test/small.webp){width=640} After' };
		let media = [{ id: 'image', public_url: 'https://img.test/source.webp', variants: [{ public_url: 'https://img.test/small.webp' }], delete_warned: confirmed }];
		function signedOut() { return false; }
		${source}
		return deleteMediaEntry(media[0]).then(() => ({ media, editor, mediaError, deletingMediaId }));
	`);
	return run(request, removeMediaImages, confirmed);
}

test('deleting media removes source and variant images from the live editor and preview', async () => {
	const state = await deleteDashboardMedia(async () => ({ ok: true }));
	assert.equal(state.media.length, 0);
	assert.equal(state.editor.content_markdown, 'Before   After');
	const dom = new JSDOM('', { url: 'https://dashboard.test' });
	try {
		const render = buildMediaRenderer(dom.window);
		dom.window.document.body.innerHTML = render(state.editor.content_markdown);
		assert.equal(dom.window.document.querySelector('img'), null);
	} finally {
		dom.window.close();
	}
});

test('failed deletion preserves images and releases the pending state', async () => {
	const state = await deleteDashboardMedia(async () => ({ ok: false, error: 'Delete failed' }));
	assert.equal(state.media.length, 1);
	assert.match(state.editor.content_markdown, /!\[photo\]/);
	assert.equal(state.mediaError, 'Delete failed');
	assert.equal(state.deletingMediaId, null);
});

test('media deletion requires confirmation before sending a request', async () => {
	const state = await deleteDashboardMedia(() => { throw new Error('must not request'); }, false);
	assert.equal(state.media[0].delete_warned, true);
	assert.match(state.editor.content_markdown, /!\[photo\]/);
});

test('media removal preserves other images, ordinary links and code examples', () => {
	const url = 'https://img.test/photo.webp';
	const markdown = `![remove](${url})\n\n[download](${url})\n\n\`![example](${url})\`\n\n\`\`\`md\n![example](${url})\n\`\`\`\n\n![keep](https://other.test/photo.webp)`;
	assert.equal(removeMediaImages(markdown, [url]), markdown.replace(`![remove](${url})`, ''));
});

// Regression tests for the dashboard's website-integration panel rules: the
// UI keeps its inputs editable and disables copying when these rules fail,
// so the rules themselves must be pinned here.

test('site addresses resolve to origins', () => {
	assert.equal(originFromSiteAddress('burakselcuk.me'), 'https://burakselcuk.me');
	assert.equal(originFromSiteAddress('https://burakselcuk.me'), 'https://burakselcuk.me');
	assert.equal(originFromSiteAddress('http://localhost:5173'), 'http://localhost:5173');
	assert.equal(originFromSiteAddress('  https://Example.com/path  '), 'https://example.com');
	assert.equal(originFromSiteAddress(''), '');
	assert.equal(originFromSiteAddress(null), '');
});

test('unusable site addresses are empty, never partial origins', () => {
	assert.equal(originFromSiteAddress('http://'), '');
	assert.equal(originFromSiteAddress('not a url'), '');
	assert.equal(originFromSiteAddress('javascript:alert(1)'), '', 'non-http schemes are rejected');
});

test('paths are a single segment or the root', () => {
	assert.equal(isValidPostPath('/blog'), true);
	assert.equal(isValidPostPath('/'), true);
	assert.equal(isValidPostPath(''), false, 'empty is not a path');
	assert.equal(isValidPostPath('/blog/posts'), false, 'multi-segment paths are rejected');
	assert.equal(isValidPostPath('/Blog'), false);
	assert.equal(isValidPostPath('blog'), false);
	assert.equal(isValidPostPath('/news/posts'), false);
});
