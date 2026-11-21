import test from 'node:test';
import assert from 'node:assert/strict';
import { isValidPostPath, originFromSiteAddress } from '../src/lib/integration.js';
import { removeMediaImages } from '../src/lib/media.js';
import { readFileSync } from 'node:fs';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { JSDOM } from 'jsdom';
import { buildMediaRenderer } from '../src/embed/media-renderer.js';
import { createBlogPages } from '../examples/customer-blog.js';

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

const post = {
	slug: 'hello-world',
	title: 'A title & "quotes" <em>not markup</em>',
	content_markdown: 'A **published** paragraph & "quotes".\n\n![Photo](/images/hello.webp){width=640}\n\n[Unsafe](javascript:alert%281%29)\n\n<script>ignored()</script>\n\n<img src=x onerror=ignored()>'
};
const secondPost = { ...post, slug: 'next-page', title: 'Another post' };

function blogPages(overrides = {}, extraPosts = []) {
	return createBlogPages({
		site: 'https://customer.test', blog: 'writing-desk', api: 'https://api.test',
		fetcher: async (input, options) => {
			const url = new URL(input);
			assert.equal(url.origin, 'https://api.test');
			assert.equal(options.credentials, 'omit');
			assert.equal(options.headers.Authorization, undefined);
			const endpoint = '/public/blogs/writing-desk/posts';
			if (url.pathname === endpoint) {
				const page = Number(url.searchParams.get('page'));
				assert.equal(url.searchParams.get('size'), '100');
				// Force the second page to keep discovery and export pagination covered.
				return Response.json({ items: page === 1 ? [post] : [secondPost, ...extraPosts], page, size: 100, total: 101 + extraPosts.length });
			}
			if (url.pathname === `${endpoint}/${post.slug}`) return Response.json(post);
			if (url.pathname === `${endpoint}/upstream-error`) return new Response(null, { status: 429 });
			return new Response(null, { status: 404 });
		}, ...overrides
	});
}

test('customer post HTML contains the article and escaped SEO metadata before JavaScript', async () => {
	const res = await blogPages({ basePath: '/news', lang: 'tr' }).respond('/news/hello-world/?source=share');
	assert.equal(res.status, 200);
	assert.match(res.headers.get('content-type'), /^text\/html/);
	const html = await res.text();
	assert.match(html, /A <strong>published<\/strong> paragraph/);
	const dom = new JSDOM(html);
	try {
		const { document } = dom.window;
		const canonical = 'https://customer.test/news/hello-world';
		assert.equal(document.documentElement.lang, 'tr');
		assert.equal(document.title, post.title);
		assert.equal(document.querySelector('article h1').textContent, post.title);
		assert.equal(document.querySelector('link[rel="canonical"]').href, canonical);
		assert.equal(document.querySelector('meta[property="og:url"]').content, canonical);
		assert.equal(document.querySelector('meta[property="og:type"]').content, 'article');
		assert.equal(document.querySelector('meta[name="description"]').content, 'A published paragraph & "quotes".');
		assert.equal(document.querySelector('meta[property="og:image"]').content, 'https://customer.test/images/hello.webp');
		assert.equal(document.querySelector('article img').width, 640);
		assert.equal(document.querySelectorAll('article img').length, 1);
		assert.equal(document.querySelector('script, [onerror], article a[href^="javascript:"], h1 em'), null);
	} finally {
		dom.window.close();
	}
});

test('customer index and sitemap link to every published post on the site origin', async () => {
	const pages = blogPages();
	const index = await pages.respond('/blog');
	assert.equal(index.status, 200);
	const html = await index.text();
	assert.match(html, /href="\/blog\/hello-world"/);
	assert.match(html, /href="\/blog\/next-page"/);
	const sitemap = await pages.respond('/blog/sitemap.xml');
	assert.match(sitemap.headers.get('content-type'), /^application\/xml/);
	const xml = await sitemap.text();
	assert.match(xml, /<loc>https:\/\/customer\.test\/blog\/<\/loc>/);
	assert.match(xml, /<loc>https:\/\/customer\.test\/blog\/hello-world<\/loc>/);
	assert.match(xml, /<loc>https:\/\/customer\.test\/blog\/next-page<\/loc>/);
	assert.doesNotMatch(html + xml, /draft|withdrawn|deleted|api\.test/);
});

test('customer routes distinguish unavailable API from missing posts and leave other routes alone', async () => {
	const pages = blogPages();
	for (const path of ['/blog/unknown', '/blog/draft', '/blog/withdrawn', '/blog/deleted', '/blog/not%2Fvalid']) {
		assert.equal((await pages.respond(path)).status, 404, path);
	}
	assert.equal((await pages.respond('/blog/upstream-error')).status, 503);
	assert.equal(await pages.respond('/about'), null);
	assert.equal((await pages.respond('/blog', 'POST')).status, 405);
	const head = await pages.respond('/blog/hello-world', 'HEAD');
	assert.equal(head.status, 200);
	assert.equal(await head.text(), '');
	const offline = blogPages({ fetcher: async () => { throw new Error('network down'); } });
	assert.equal((await offline.respond('/blog/sitemap.xml')).status, 503);
});

test('root posts, blog index and existing site routes stay separate in SSR and export', async () => {
	const pages = blogPages({ basePath: '/', indexPath: '/blog', excludedSlugs: ['about'] }, [
		{ ...post, slug: 'about', title: 'Reserved site page' },
		{ ...post, slug: 'blog', title: 'Conflicting index slug' }
	]);
	assert.equal(await pages.respond('/'), null);
	assert.equal(await pages.respond('/about/?x=1'), null);
	assert.equal((await pages.respond('/missing')).status, 404);
	const article = await pages.respond('/hello-world');
	assert.equal(article.status, 200);
	const html = await article.text();
	assert.match(html, /rel="canonical" href="https:\/\/customer\.test\/hello-world"/);
	assert.match(html, /<nav><a href="\/blog\/">/);
	const index = await pages.respond('/blog');
	assert.equal(index.status, 200);
	const indexHtml = await index.text();
	assert.match(indexHtml, /href="\/hello-world"/);
	assert.doesNotMatch(indexHtml, /Reserved site page|Conflicting index slug/);
	const sitemap = await (await pages.respond('/blog/sitemap.xml')).text();
	assert.match(sitemap, /<loc>https:\/\/customer\.test\/blog\/<\/loc>/);
	assert.match(sitemap, /<loc>https:\/\/customer\.test\/hello-world<\/loc>/);
	assert.doesNotMatch(sitemap, /\/about|\/blog<\/loc>/);

	const temp = await mkdtemp(join(tmpdir(), 'statesu-root-blog-'));
	try {
		const output = join(temp, 'fresh');
		await pages.exportTo(output);
		await assert.rejects(readFile(join(output, 'index.html')), { code: 'ENOENT' });
		await assert.rejects(readFile(join(output, 'about/index.html')), { code: 'ENOENT' });
		assert.match(await readFile(join(output, 'blog/index.html'), 'utf8'), /href="\/hello-world\/"/);
		assert.match(await readFile(join(output, 'blog/sitemap.xml'), 'utf8'), /<loc>https:\/\/customer\.test\/hello-world\/<\/loc>/);
		assert.match(await readFile(join(output, 'hello-world/index.html'), 'utf8'), /rel="canonical" href="https:\/\/customer\.test\/hello-world\/"/);
	} finally {
		await rm(temp, { recursive: true, force: true });
	}
});

test('static export uses directory canonicals and never overwrites existing output', async () => {
	const temp = await mkdtemp(join(tmpdir(), 'statesu-blog-'));
	try {
		const output = join(temp, 'fresh');
		const pages = blogPages();
		await pages.exportTo(output);
		const index = await readFile(join(output, 'blog/index.html'), 'utf8');
		const postHtml = await readFile(join(output, 'blog/hello-world/index.html'), 'utf8');
		const sitemap = await readFile(join(output, 'blog/sitemap.xml'), 'utf8');
		assert.match(index, /href="\/blog\/hello-world\/"/);
		assert.match(postHtml, /rel="canonical" href="https:\/\/customer\.test\/blog\/hello-world\/"/);
		assert.match(sitemap, /<loc>https:\/\/customer\.test\/blog\/next-page\/<\/loc>/);
		await assert.rejects(readFile(join(output, 'blog/withdrawn/index.html')), { code: 'ENOENT' });
		await assert.rejects(pages.exportTo(output), { code: 'EEXIST' });
	} finally {
		await rm(temp, { recursive: true, force: true });
	}
});
