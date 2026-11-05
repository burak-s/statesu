import { test as base, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { execFileSync, spawn } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const API = `http://localhost:${process.env.STATESU_BROWSER_API_PORT ?? '4180'}`;
const HOST = `http://127.0.0.1:${process.env.STATESU_BROWSER_HOST_PORT ?? '4181'}`;
const PASSWORD = 'browser password 7';

async function json(response, status = 200) {
	expect(response.status(), await response.text()).toBe(status);
	return response.json();
}

async function registerAccount(request) {
	const email = `browser-${randomUUID()}@example.com`;
	const headers = { Origin: API };
	await json(await request.post(`${API}/auth/register`, {
		headers, data: { email, password: PASSWORD }
	}), 202);
	// Only mail delivery is replaced: registration, verification, sessions,
	// PATs and all content operations still cross the real HTTP API.
	const mailbox = await json(await request.get(`${API}/__test/mail`, { params: { email } }));
	expect(mailbox.links).toHaveLength(1);
	const token = new URL(mailbox.links[0]).searchParams.get('token');
	await json(await request.post(`${API}/auth/verify`, { headers, data: { token } }));
	const login = await request.post(`${API}/auth/login`, {
		headers, data: { email, password: PASSWORD }
	});
	await json(login);
	const cookies = login.headersArray()
		.filter(({ name }) => name.toLowerCase() === 'set-cookie')
		.map(({ value }) => value.split(';')[0]);
	const csrf = cookies.find((cookie) => cookie.startsWith('__Host-statesu_csrf=')).split('=')[1];
	const pat = await json(await request.post(`${API}/auth/pats`, {
		headers: { ...headers, Cookie: cookies.join('; '), 'X-CSRF-Token': csrf },
		data: { name: 'Browser tests', write: true }
	}), 201);
	return { email, token: pat.secret };
}

const test = base.extend({
	account: async ({ request }, use) => {
		const account = await registerAccount(request);
		account.headers = { Authorization: `Bearer ${account.token}` };
		const blog = await json(await request.post(`${API}/blogs`, {
			headers: account.headers,
			data: { name: 'Browser blog', slug: `browser-${randomUUID().slice(0, 8)}` }
		}), 201);
		await json(await request.put(`${API}/blogs/${blog.id}/origins`, {
			headers: account.headers,
			data: { origins: [HOST, 'https://existing.example'] }
		}));
		await use({ ...account, blog });
	}
});

function postUrl(account, slug, options = {}) {
	const url = new URL(options.path ?? `/blog/${slug}`, HOST);
	url.searchParams.set('blog', account.blog.slug);
	for (const [name, value] of Object.entries(options)) {
		if (name !== 'path') url.searchParams.set(name, value);
	}
	return url.href;
}

async function createPost(request, account, slug, overrides = {}) {
	return json(await request.post(`${API}/blogs/${account.blog.id}/posts`, {
		headers: account.headers,
		data: { slug, title: `Post ${slug}`, content_markdown: `# ${slug}\n\n**Published content**`, ...overrides }
	}), 201);
}

async function patchPost(request, account, post, data) {
	return json(await request.patch(`${API}/blogs/${account.blog.id}/posts/${post.id}`, {
		headers: account.headers, data
	}));
}

async function signIn(page, account) {
	await page.goto(`${API}/dashboard`);
	await page.getByLabel('Email', { exact: true }).fill(account.email);
	await page.getByLabel('Password', { exact: true }).fill(PASSWORD);
	await page.getByRole('button', { name: 'Sign in', exact: true }).click();
	await expect(page.getByRole('navigation', { name: 'Sections' })).toBeVisible();
}

test('CLI login signs in on the website, approves access, exchanges PKCE code and logs out', async ({ page, request, account }) => {
	test.skip(process.platform !== 'linux', 'isolated browser launcher fixture uses xdg-open');
	const directory = await mkdtemp(join(tmpdir(), 'stscli-browser-'));
	const root = fileURLToPath(new URL('../../../', import.meta.url));
	const binary = resolve(root, process.env.CARGO_TARGET_DIR ?? 'target', 'debug/stscli');
	const legacyBinary = resolve(root, process.env.CARGO_TARGET_DIR ?? 'target', 'debug/statesu-cli');
	// Do not launch an external browser: Playwright opens the printed link.
	await writeFile(join(directory, 'xdg-open'), '#!/bin/sh\nexit 0\n', { mode: 0o700 });
	const env = { ...process.env, XDG_CONFIG_HOME: directory, PATH: `${directory}:${process.env.PATH}` };
	delete env.STATESU_TOKEN;
	delete env.STATESU_SERVER;
	const child = spawn(binary, ['login', '--server', API], { env });
	let stderr = '';
	let stdout = '';
	child.stderr.on('data', (chunk) => { stderr += chunk; });
	child.stdout.on('data', (chunk) => { stdout += chunk; });
	const completion = new Promise((resolve, reject) => {
		child.once('error', reject);
		child.once('exit', (code) => resolve(code));
	});
	try {
		await expect.poll(() => stderr.includes('/cli/authorize?')).toBe(true);
		const link = stderr.match(/http:\/\/[^\s]+\/cli\/authorize\?[^\s]+/)[0];
		const authorization = new URL(link);
		const callback = authorization.searchParams.get('redirect_uri');
		// Incorrect state must not finish or consume the pending login.
		const bad = new URL(callback);
		bad.searchParams.set('state', '0'.repeat(64));
		bad.searchParams.set('code', 'su_cli_' + 'a'.repeat(64));
		expect((await request.get(bad.href)).status()).toBe(400);
		await page.goto(link);
		await page.getByRole('link', { name: 'Sign in to continue' }).click();
		await page.getByLabel('Email', { exact: true }).fill(account.email);
		await page.getByLabel('Password', { exact: true }).fill(PASSWORD);
		await page.getByRole('button', { name: 'Sign in', exact: true }).click();
		await expect(page).toHaveURL(link);
		await expect(page.getByText(account.email, { exact: true })).toBeVisible();
		await page.getByRole('button', { name: 'Authorize CLI', exact: true }).click();
		await expect(page.getByRole('heading', { name: 'Authorization received' })).toBeVisible();
		expect(await completion).toBe(0);
		expect(stdout).toContain('Logged in via browser.');
		const settings = JSON.parse(await readFile(join(directory, 'statesu/cli.json'), 'utf8'));
		expect(settings.pat).toMatch(/^su_pat_/);
		expect(stdout).not.toContain(settings.pat);
		expect(stderr).not.toContain(settings.pat);
		const whoami = JSON.parse(execFileSync(binary, ['whoami', '--json'], { env, encoding: 'utf8' }));
		expect(whoami.blogs.some((blog) => blog.id === account.blog.id)).toBe(true);
		const legacyWhoami = JSON.parse(execFileSync(legacyBinary, ['whoami', '--json'], { env, encoding: 'utf8' }));
		expect(legacyWhoami).toEqual(whoami);
		execFileSync(binary, ['logout'], { env });
		expect((await request.get(`${API}/blogs`, {
			headers: { Authorization: `Bearer ${settings.pat}` }
		})).status()).toBe(401);
	} finally {
		child.kill();
		await rm(directory, { recursive: true, force: true });
	}
});

test('dashboard makes token revocation confirmation explicit and revokes access immediately', async ({ page, request, account }) => {
	await signIn(page, account);
	await page.getByRole('button', { name: 'API tokens', exact: true }).click();
	const row = page.getByRole('row').filter({ hasText: 'Browser tests' });
	await row.getByRole('button', { name: 'Revoke', exact: true }).click();
	await expect(page.getByRole('alert')).toContainText('Clients using it stop working immediately.');
	expect((await request.get(`${API}/blogs`, { headers: account.headers })).status()).toBe(200);
	await expect(row.getByRole('button', { name: 'Confirm revocation', exact: true })).toBeVisible();
	await row.getByRole('button', { name: 'Cancel', exact: true }).click();
	await expect(page.getByRole('alert')).toHaveCount(0);
	await row.getByRole('button', { name: 'Revoke', exact: true }).click();
	await row.getByRole('button', { name: 'Confirm revocation', exact: true }).click();
	await expect(row).toHaveCount(0);
	expect((await request.get(`${API}/blogs`, { headers: account.headers })).status()).toBe(401);
});

test('CLI authorization rejects external callbacks before offering sign-in', async ({ page }) => {
	await page.goto(`${API}/cli/authorize?redirect_uri=https://evil.example/callback&code_challenge=${'a'.repeat(43)}&state=${'b'.repeat(64)}`);
	await expect(page.getByRole('alert')).toContainText('Invalid CLI authorization link');
	await expect(page.getByRole('link', { name: 'Sign in to continue' })).toHaveCount(0);
});

test('real bundle resolves deep links, refresh, future posts and the root prefix across origins', async ({ page, context, request, account }) => {
	await createPost(request, account, 'test', { status: 'published' });
	await createPost(request, account, 'deneme', { status: 'published' });
	await context.addCookies([{ name: 'must-not-send', value: 'secret', url: API }]);
	const deliveries = [];
	page.on('request', (request) => {
		if (request.url().includes('/public/blogs/')) deliveries.push(request);
	});
	for (const slug of ['test', 'deneme']) {
		await page.goto(postUrl(account, slug));
		await expect(page.locator('.statesu-title')).toHaveText(`Post ${slug}`);
		await expect(page.locator('.statesu-body strong')).toHaveText('Published content');
		await page.reload();
		await expect(page.locator('.statesu-title')).toHaveText(`Post ${slug}`);
	}
	await createPost(request, account, 'future', { status: 'published' });
	await page.goto(postUrl(account, 'future'));
	await expect(page.locator('.statesu-title')).toHaveText('Post future');
	await page.goto(postUrl(account, 'test', { path: '/test', base: '/' }));
	await expect(page.locator('.statesu-title')).toHaveText('Post test');
	await expect(page.locator('#host-header')).toHaveText('Host navigation');
	await expect(page.locator('#host-footer')).toHaveText('Host footer');
	expect(deliveries.length).toBeGreaterThanOrEqual(6);
	for (const delivery of deliveries) {
		const headers = await delivery.allHeaders();
		expect(headers.origin).toBe(HOST);
		expect(headers.authorization).toBeUndefined();
		expect(headers.cookie).toBeUndefined();
		const response = await delivery.response();
		expect(response.status()).toBe(200);
		expect(response.headers()['access-control-allow-origin']).toBe(HOST);
	}
});

test('authenticated stored XSS stays inert through update, publish, dashboard and embed', async ({ page, context, request, account }) => {
	const dialogs = [];
	const trackers = [];
	const errors = [];
	await context.addInitScript(() => { window.__xss = false; });
	page.on('dialog', async (dialog) => { dialogs.push(dialog.message()); await dialog.dismiss(); });
	page.on('pageerror', (error) => errors.push(error.message));
	context.on('request', (request) => {
		if (new URL(request.url()).pathname === '/tracker') trackers.push(request.url());
	});
	const payload = `# Safe heading

<script>window.__xss = true; alert('stored-xss')</script>
<img src="${HOST}/tracker" onerror="window.__xss=true">
<svg onload="window.__xss=true"><script>alert('svg')</script></svg>
<math><mtext><img src="${HOST}/tracker" onerror="window.__xss=true"></mtext></math>
<iframe src="${HOST}/tracker"></iframe>
<form id="statesu-post" name="__STATESU_EMBED_V1__"><input autofocus onfocus="window.__xss=true"></form>
<style>body{background:url('${HOST}/tracker')}</style>

[bad](javascript:alert%281%29) [entity](java&#115;cript:alert%281%29)
[data](data:text/html,boom) [contact](mailto:help@example.com)
![tracker](${HOST}/tracker)

[Safe link](/about) **normal formatting**`;
	const title = '<img src=x onerror="window.__xss=true">';
	const post = await createPost(request, account, 'stored-xss', { title, content_markdown: payload });
	const updated = `${payload}\n\nUpdated through the API.`;
	await patchPost(request, account, post, { content_markdown: updated });
	await patchPost(request, account, post, { status: 'published' });
	const publicPost = await json(await request.get(`${API}/public/blogs/${account.blog.slug}/posts/stored-xss`));
	expect(publicPost.content_markdown).toBe(updated);

	await signIn(page, account);
	await page.getByRole('button', { name: 'Browser blog', exact: true }).click();
	const row = page.getByRole('row').filter({ has: page.getByText('stored-xss', { exact: true }) });
	await expect(row).toContainText(title);
	await row.getByRole('button', { name: 'Edit', exact: true }).click();
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue(title);
	await expect(page.getByLabel('Markdown', { exact: true })).toHaveValue(updated);
	await expect(page.locator('main img, main svg, main math, main iframe')).toHaveCount(0);
	expect(await page.evaluate(() => window.__xss)).toBe(false);

	await page.goto(postUrl(account, 'stored-xss'));
	await expect(page.locator('.statesu-title')).toHaveText(title);
	await expect(page.locator('.statesu-body')).toContainText('Updated through the API.');
	await expect(page.locator('.statesu-body strong')).toHaveText('normal formatting');
	await expect(page.locator('.statesu-body')).toContainText('contact');
	const body = page.locator('.statesu-body');
	await expect(body.locator('script, img, svg, math, iframe, form, input, style, [id], [name]')).toHaveCount(0);
	const attributes = await body.locator('*').evaluateAll((nodes) => nodes.flatMap((node) => node.getAttributeNames()));
	expect(attributes.every((name) => ['href', 'title', 'target', 'rel'].includes(name))).toBe(true);
	const links = await body.locator('a').evaluateAll((nodes) => nodes.map((node) => ({
		href: node.href, target: node.target, rel: node.rel
	})));
	expect(links).toEqual([{ href: `${HOST}/about`, target: '_blank', rel: 'noopener noreferrer' }]);
	expect(await page.evaluate(() => window.__xss)).toBe(false);
	expect(dialogs).toEqual([]);
	expect(trackers).toEqual([]);
	expect(errors).toEqual([]);
});

test('dashboard keeps invalid inputs editable, gates copying and adds origins only on Save', async ({ page, context, request, account }) => {
	await signIn(page, account);
	await page.getByRole('button', { name: 'Settings for Browser blog', exact: true }).click();
	const origin = page.getByLabel('Website address', { exact: true });
	const path = page.getByLabel('Posts path', { exact: true });
	const copy = page.getByRole('button', { name: 'Copy code', exact: true });
	await origin.fill('http://');
	await expect(page.getByRole('alert')).toContainText('Enter a valid site address');
	await expect(origin).toBeVisible();
	await expect(origin).toBeEditable();
	await expect(copy).toBeDisabled();
	await origin.fill(HOST);
	await path.fill('/news/posts');
	await expect(page.getByRole('alert')).toContainText('single lowercase segment');
	await expect(path).toBeEditable();
	await expect(copy).toBeDisabled();
	await path.fill('/');
	await expect(copy).toBeEnabled();
	await page.getByText('Copyable integration code', { exact: true }).click();
	const snippet = page.locator('.details-body pre code');
	await expect(snippet).toContainText(`src="${API}/embed/v1.js"`);
	await expect(snippet).toContainText('data-base-path="/"');
	await expect(snippet).toContainText(`data-blog="${account.blog.slug}"`);
	await context.grantPermissions(['clipboard-read', 'clipboard-write']);
	await copy.click();
	await expect(page.getByRole('button', { name: 'Copied!', exact: true })).toBeVisible();
	expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(await snippet.textContent());

	await origin.fill('https://new.example');
	await page.getByRole('button', { name: 'Add site origin to the list', exact: true }).click();
	const origins = page.getByLabel('Allowed origins', { exact: true });
	await expect(origins).toHaveValue(`${HOST}\nhttps://existing.example\nhttps://new.example`);
	const before = await json(await request.get(`${API}/blogs`, { headers: account.headers }));
	expect(before.find((blog) => blog.id === account.blog.id).origins).not.toContain('https://new.example');
	await page.getByRole('button', { name: 'Save settings', exact: true }).click();
	await expect(page.getByText('Settings saved.', { exact: true })).toBeVisible();
	const saved = await json(await request.get(`${API}/blogs`, { headers: account.headers }));
	expect(saved.find((blog) => blog.id === account.blog.id).origins).toEqual(expect.arrayContaining([HOST, 'https://existing.example', 'https://new.example']));
});

test('unsupported sanitizer renders the stored Markdown as text, never HTML', async ({ page, request, account }) => {
	const markdown = '# Not HTML\n\n[click](javascript:alert%281%29)\n\n<img src=x onerror="window.__xss=true">';
	await createPost(request, account, 'unsupported', { content_markdown: markdown, status: 'published' });
	await page.addInitScript(() => { window.__xss = false; });
	await page.goto(postUrl(account, 'unsupported', { unsupported: '1' }));
	const fallback = page.locator('pre.statesu-embed-fallback');
	await expect(fallback).toHaveText(markdown);
	await expect(fallback.locator('*')).toHaveCount(0);
	await expect(page.locator('#statesu-post a, #statesu-post img')).toHaveCount(0);
	expect(await page.evaluate(() => window.__xss)).toBe(false);
});

test('drafts, withdrawn and deleted posts stay private and foreign mutations are rejected', async ({ page, request, account }) => {
	const post = await createPost(request, account, 'lifecycle');
	const publicUrl = `${API}/public/blogs/${account.blog.slug}/posts/lifecycle`;
	expect((await request.get(publicUrl)).status()).toBe(404);
	const shell = await page.goto(postUrl(account, 'lifecycle'));
	expect(shell.status()).toBe(200); // A client-side 404 cannot change the host's HTTP status.
	await expect(page.getByRole('status')).toHaveText('No published post at this address.');
	await patchPost(request, account, post, { status: 'published' });
	await page.reload();
	await expect(page.locator('.statesu-title')).toHaveText('Post lifecycle');

	const outsider = await registerAccount(request);
	const foreignHeaders = { Authorization: `Bearer ${outsider.token}` };
	expect((await request.get(`${API}/blogs/${account.blog.id}/posts`, { headers: foreignHeaders })).status()).toBe(404);
	expect((await request.patch(`${API}/blogs/${account.blog.id}/posts/${post.id}`, {
		headers: foreignHeaders, data: { status: 'draft', title: 'Stolen' }
	})).status()).toBe(404);
	expect((await request.delete(`${API}/blogs/${account.blog.id}/posts/${post.id}`, { headers: foreignHeaders })).status()).toBe(404);
	expect((await json(await request.get(publicUrl))).title).toBe('Post lifecycle');

	await patchPost(request, account, post, { status: 'draft' });
	expect((await request.get(publicUrl)).status()).toBe(404);
	await page.reload();
	await expect(page.getByRole('status')).toHaveText('No published post at this address.');
	await patchPost(request, account, post, { status: 'published' });
	expect((await request.delete(`${API}/blogs/${account.blog.id}/posts/${post.id}`, { headers: account.headers })).status()).toBe(204);
	expect((await request.get(publicUrl)).status()).toBe(404);
	await page.reload();
	await expect(page.getByRole('status')).toHaveText('No published post at this address.');
});

test('invalid URLs never fetch and pages outside the prefix remain untouched', async ({ page, account }) => {
	const deliveries = [];
	page.on('request', (request) => {
		if (request.url().includes('/public/blogs/')) deliveries.push(request.url());
	});
	for (const path of ['/blog/a/b', '/blog/a%2Fb', '/blog/%252F', '/blog/%E2%28', '/blog/Test', '/blog/test//']) {
		await page.goto(postUrl(account, '', { path }));
		await expect(page.getByRole('alert')).toHaveText('This URL is not a valid blog post address.');
	}
	await page.goto(postUrl(account, '', { path: '/blog' }));
	await expect(page.getByRole('status')).toContainText('This page hosts blog posts.');
	for (const path of ['/blogger', '/outside/page']) {
		await page.goto(postUrl(account, '', { path }));
		await expect(page.locator('#statesu-post')).toHaveText('Host placeholder');
		await expect(page.locator('.statesu-embed, style[data-statesu]')).toHaveCount(0);
	}
	expect(deliveries).toEqual([]);
});

test('the browser enforces exact-origin CORS and missing targets do not mutate the host', async ({ page, request, account }) => {
	await createPost(request, account, 'cors', { status: 'published' });
	await json(await request.put(`${API}/blogs/${account.blog.id}/origins`, {
		headers: account.headers, data: { origins: ['http://127.0.0.1:1'] }
	}));
	await page.goto(postUrl(account, 'cors'));
	await expect(page.getByRole('alert')).toContainText('could not be reached');
	await expect(page.getByRole('alert')).toContainText('site origin must be allowed');
	await expect(page.locator('.statesu-title')).toHaveCount(0);
	await json(await request.put(`${API}/blogs/${account.blog.id}/origins`, {
		headers: account.headers, data: { origins: [HOST] }
	}));
	await page.reload();
	await expect(page.locator('.statesu-title')).toHaveText('Post cors');
	const deliveries = [];
	page.on('request', (request) => {
		if (request.url().includes('/public/blogs/')) deliveries.push(request.url());
	});
	await page.goto(postUrl(account, 'cors', { target: 'missing' }));
	await expect(page.locator('#statesu-post')).toHaveText('Host placeholder');
	await expect(page.locator('.statesu-embed')).toHaveCount(0);
	expect(deliveries).toEqual([]);
});

test('network failure is safe and the production deadline cancels a stalled browser request', async ({ page, account }) => {
	// Fault injection at the HTTP boundary, never at window.fetch or init().
	const delivery = `${API}/public/blogs/${account.blog.slug}/posts/network`;
	await page.route(delivery, (route) => route.abort('failed'));
	await page.goto(postUrl(account, 'network'));
	await expect(page.getByRole('alert')).toContainText('could not be reached');
	await page.unroute(delivery);
	await page.route(delivery, () => {}); // No response: leave the real fetch pending.
	const cancelled = page.waitForEvent('requestfailed', {
		predicate: (request) => request.url() === delivery
	});
	await page.reload();
	await expect(page.getByRole('status')).toHaveText('Loading post…');
	await expect(page.getByRole('alert')).toContainText('did not answer in time', { timeout: 15_000 });
	expect((await cancelled).failure().errorText).toContain('ERR_ABORTED');
	await expect(page.locator('.statesu-title')).toHaveCount(0);
});
