// Runs on the blog owner's host or at build time.
import { mkdir, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import { JSDOM } from 'jsdom';
import { FETCH_TIMEOUT_MS, fetchPostJson, validBlogSlug } from '../src/embed/common.js';
import { buildMediaRenderer } from '../src/embed/media-renderer.js';
import { parseBasePath, resolveSlug } from '../src/embed/url.js';

function origin(value) {
	const url = new URL(value);
	if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.href !== `${url.origin}/`) {
		throw new Error('Expected an HTTP(S) origin, e.g. https://example.com');
	}
	return url.origin;
}

function escapeMarkup(value) {
	return value.replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[char]);
}

function validPost(post) {
	return validBlogSlug(post?.slug) && typeof post.title === 'string' && typeof post.content_markdown === 'string';
}

// The HTML renderer can sit under a nested path without changing the embed's
// single-segment configuration or the dashboard validation.
function parsePagePath(value) {
	if (typeof value === 'string' && /^\/[a-z0-9-]+(?:\/[a-z0-9-]+)+\/?$/.test(value)) {
		return value.replace(/\/$/, '');
	}
	return parseBasePath(value);
}

export function createBlogPages({
	site, blog, api = 'https://statesu.com', basePath, indexPath, excludedSlugs = [], lang = 'en',
	fetcher = globalThis.fetch, title = 'Blog', description = 'Published posts', stylesheet
}) {
	if (!validBlogSlug(blog)) throw new Error('Expected a Statesu blog slug');
	const siteOrigin = origin(site);
	if (!/^[a-zA-Z]{2,8}(?:-[a-zA-Z0-9]{1,8})*$/.test(lang)) throw new Error('Expected a language tag, e.g. en or tr');
	const base = parsePagePath(basePath);
	const prefix = base === '/' ? '' : base;
	const index = indexPath === undefined ? base : parsePagePath(indexPath);
	const indexDirectory = index === '/' ? '/' : `${index}/`;
	const postsApi = `${origin(api)}/public/blogs/${blog}/posts`;
	const sitemapPath = `${indexDirectory}sitemap.xml`;
	if (!Array.isArray(excludedSlugs) || !excludedSlugs.every(validBlogSlug)) {
		throw new Error('Expected excluded post slugs, e.g. about or announced');
	}
	const excluded = new Set(excludedSlugs);
	const pagePath = (slug, directoryUrls) => slug
		? `${prefix}/${slug}${directoryUrls ? '/' : ''}`
		: indexDirectory;
	const canServePost = slug => !excluded.has(slug) && pagePath(slug, false) !== index;
	const canonical = (slug, directoryUrls) => `${siteOrigin}${pagePath(slug, directoryUrls)}`;
	const load = url => fetchPostJson(fetcher, url, FETCH_TIMEOUT_MS);

	async function listPosts() {
		const posts = new Map();
		const size = 100;
		for (let page = 1; ; page += 1) {
			const result = await load(`${postsApi}?page=${page}&size=${size}`);
			if (result.error?.status === 404) return null;
			const listing = result.post;
			if (!Array.isArray(listing?.items) || !listing.items.every(validPost) ||
				!Number.isSafeInteger(listing.total) || listing.total < 0 || listing.page !== page || listing.size !== size) {
				throw new Error('Could not load published posts');
			}
			for (const post of listing.items) {
				if (canServePost(post.slug)) posts.set(post.slug, post);
			}
			if (page * size >= listing.total || listing.items.length === 0) return [...posts.values()];
		}
	}

	function html({ title, description, url, body, article = false, image }) {
		const text = escapeMarkup(title);
		const summary = escapeMarkup(description);
		const address = escapeMarkup(url);
		return `<!doctype html>
<html lang="${lang}"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${text}</title><meta name="description" content="${summary}">
<link rel="canonical" href="${address}">
<meta property="og:type" content="${article ? 'article' : 'website'}">
<meta property="og:title" content="${text}"><meta property="og:description" content="${summary}">
<meta property="og:url" content="${address}">${image ? `\n<meta property="og:image" content="${escapeMarkup(image)}">` : ''}
${stylesheet ? `<link rel="stylesheet" href="${escapeMarkup(stylesheet)}">\n` : ''}</head><body><main>${body}</main></body></html>`;
	}

	function renderIndex(posts, directoryUrls) {
		const links = posts.map(post => `<li><a href="${pagePath(post.slug, directoryUrls)}">${escapeMarkup(post.title)}</a></li>`).join('\n');
		return html({ title, description, url: canonical(null, directoryUrls), body: `<h1>${escapeMarkup(title)}</h1><p>${escapeMarkup(description)}</p><ul>${links}</ul>` });
	}

	function renderPost(post, directoryUrls) {
		const url = canonical(post.slug, directoryUrls);
		const { window } = new JSDOM('', { url });
		try {
			// This is the same sanitizer used by v2; JSDOM does not run scripts or load subresources by default.
			const render = buildMediaRenderer(window);
			const content = render ? render(post.content_markdown) : `<pre>${escapeMarkup(post.content_markdown)}</pre>`;
			const article = window.document.createElement('div');
			article.innerHTML = content;
			const excerpt = article.querySelector('p')?.textContent?.trim() || article.textContent.trim() || post.title;
			const description = [...excerpt.replace(/\s+/g, ' ')].slice(0, 160).join('');
			const body = `<nav><a href="${pagePath(null, directoryUrls)}">${escapeMarkup(title)}</a></nav><article><h1>${escapeMarkup(post.title)}</h1>${content}</article>`;
			return html({ title: post.title, description, url, body, article: true, image: article.querySelector('img')?.getAttribute('src') });
		} finally {
			window.close();
		}
	}

	function renderSitemap(posts, directoryUrls) {
		const urls = [null, ...posts.map(post => post.slug)].map(slug => `<url><loc>${escapeMarkup(canonical(slug, directoryUrls))}</loc></url>`);
		return `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls.join('\n')}\n</urlset>\n`;
	}

	async function respond(path, method = 'GET', directoryUrls = false) {
		// Use the configured origin, not Host or X-Forwarded-Host, for metadata.
		if (typeof path !== 'string' || !path.startsWith('/') || path.startsWith('//')) {
			return new Response(null, { status: 400 });
		}
		const requestUrl = new URL(siteOrigin + path);
		const sitemap = requestUrl.pathname === sitemapPath;
		const isIndex = requestUrl.pathname === index || requestUrl.pathname === indexDirectory;
		const route = resolveSlug(requestUrl, base);
		if (!sitemap && !isIndex && (route.kind === 'skip' || route.kind === 'empty' ||
			(route.kind === 'post' && !canServePost(route.slug)))) return null;
		const reply = (status, body, type = 'text/plain', headers = {}) => new Response(method === 'HEAD' ? null : body, {
			status, headers: { 'Content-Type': `${type}; charset=utf-8`, 'Cache-Control': 'no-store', ...headers }
		});
		const notFound = () => reply(404, 'Not found');
		const unavailable = () => reply(503, 'Blog temporarily unavailable');
		if (method !== 'GET' && method !== 'HEAD') return reply(405, 'Method not allowed', 'text/plain', { Allow: 'GET, HEAD' });
		try {
			if (sitemap || isIndex) {
				const posts = await listPosts();
				if (posts === null) return notFound();
				return sitemap ? reply(200, renderSitemap(posts, directoryUrls), 'application/xml') : reply(200, renderIndex(posts, directoryUrls), 'text/html');
			}
			if (route.kind !== 'post') return notFound();
			const { post, error } = await load(`${postsApi}/${route.slug}`);
			if (error?.status === 404) return notFound();
			if (!validPost(post) || post.slug !== route.slug) return unavailable();
			return reply(200, renderPost(post, directoryUrls), 'text/html');
		} catch {
			return unavailable();
		}
	}

	async function exportTo(output) {
		const posts = await listPosts();
		if (posts === null) throw new Error('Blog not found');
		// Never overwrite a customer site or reuse stale files from a previous build.
		await mkdir(output);
		const indexOutput = join(output, index.slice(1));
		await mkdir(indexOutput, { recursive: true });
		await writeFile(join(indexOutput, 'index.html'), renderIndex(posts, true));
		await writeFile(join(indexOutput, 'sitemap.xml'), renderSitemap(posts, true));
		const postsOutput = join(output, prefix.slice(1));
		await mkdir(postsOutput, { recursive: true });
		for (const post of posts) {
			const folder = join(postsOutput, post.slug);
			await mkdir(folder);
			await writeFile(join(folder, 'index.html'), renderPost(post, true));
		}
	}

	return { respond, exportTo };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
	const { values } = parseArgs({ options: {
		site: { type: 'string' }, blog: { type: 'string' }, api: { type: 'string' },
		'base-path': { type: 'string' }, 'index-path': { type: 'string' },
		'exclude-slug': { type: 'string', multiple: true }, lang: { type: 'string' },
		port: { type: 'string', default: '3000' }, export: { type: 'string' }
	} });
	const pages = createBlogPages({
		...values, basePath: values['base-path'], indexPath: values['index-path'], excludedSlugs: values['exclude-slug']
	});
	if (values.export) {
		await pages.exportTo(values.export);
	} else {
		createServer(async (request, response) => {
			const result = await pages.respond(request.url, request.method) ?? new Response(null, { status: 404 });
			response.writeHead(result.status, Object.fromEntries(result.headers));
			response.end(Buffer.from(await result.arrayBuffer()));
		}).listen(Number(values.port), '127.0.0.1', () => {
			console.log(`Blog listening on http://127.0.0.1:${values.port}`);
		});
	}
}
