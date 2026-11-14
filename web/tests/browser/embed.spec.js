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

test('saved appearance styles the v2 embed while v1 and host overrides keep their rules', async ({ page, request, account }) => {
	await createPost(request, account, 'styled', {
		status: 'published',
		content_markdown: 'Text with a [link](https://example.com) inside.\n\n# Styled heading\n\nA paragraph for the rhythm.'
	});
	await json(await request.put(`${API}/blogs/${account.blog.id}/appearance`, {
		headers: account.headers,
		data: {
			embed_appearance: {
				version: 1,
				colors: { text: '#123456', link: '#ff0000' },
				typography: { heading_scale: 200 },
				layout: { spacing: 32 }
			}
		}
	}));

	// The saved appearance applies inside the shadow root.
	await page.goto(postUrl(account, 'styled', { v: '2', style: '#statesu-post { font-family: Georgia, serif; }' }));
	const title = page.locator('.statesu-title');
	await expect(title).toHaveText('Post styled');
	await expect.poll(() => title.evaluate((el) => getComputedStyle(el).color)).toBe('rgb(18, 52, 86)');
	const link = page.locator('.statesu-body a');
	await expect(link).toHaveText('link');
	await expect.poll(() => link.evaluate((el) => getComputedStyle(el).color)).toBe('rgb(255, 0, 0)');
	const fontFamily = await title.evaluate((el) => getComputedStyle(el).fontFamily);
	expect(fontFamily).toContain('Georgia');
	// Heading scale multiplies every Markdown heading, and the spacing
	// token drives the paragraph rhythm.
	const heading = page.locator('.statesu-body h1');
	await expect(heading).toHaveText('Styled heading');
	await expect.poll(() => heading.evaluate((el) => getComputedStyle(el).fontSize)).toBe('64px');
	const paragraph = page.locator('.statesu-body p').last();
	await expect.poll(() => paragraph.evaluate((el) => getComputedStyle(el).marginBlockStart)).toBe('32px');

	// Documented host-page overrides beat the saved appearance.
	await page.goto(postUrl(account, 'styled', {
		v: '2',
		style: '#statesu-post { --statesu-link-color: #00ff00; --statesu-font-family: monospace; }'
	}));
	const overridden = page.locator('.statesu-body a');
	await expect(overridden).toHaveText('link');
	await expect(page.locator('.statesu-title')).toHaveCSS('font-family', 'monospace');
	await expect.poll(() => overridden.evaluate((el) => getComputedStyle(el).color)).toBe('rgb(0, 255, 0)');

	// v1 embeds ignore the appearance: their look stays host-page-driven.
	await page.goto(postUrl(account, 'styled'));
	const v1Title = page.locator('.statesu-title');
	await expect(v1Title).toHaveText('Post styled');
	await expect.poll(() => v1Title.evaluate((el) => getComputedStyle(el).color)).not.toBe('rgb(18, 52, 86)');
});

test('the appearance editor saves typed values and the preview matches the embed', async ({ page, request, account }) => {
	await signIn(page, account);
	await page.getByRole('button', { name: 'Settings for Browser blog', exact: true }).click();
	const section = page.locator('.appearance');
	await expect(section).toBeVisible();

	// Presets work: Editorial fills the draft without crashing.
	await section.getByRole('button', { name: 'Editorial', exact: true }).click();
	await expect(section.getByLabel('Body size (%)', { exact: true })).toHaveValue('');

	// Numeric tokens are submitted as numbers; the backend rejects strings.
	const bodySize = section.getByLabel('Body size (%)', { exact: true });
	await bodySize.fill('110');
	const textColor = section.getByLabel('Text color', { exact: true });
	await textColor.fill('#123456');
	await expect(section.getByRole('button', { name: 'Save appearance' })).toBeEnabled();
	await section.getByRole('button', { name: 'Save appearance', exact: true }).click();
	await expect(section.getByRole('status')).toContainText('Appearance saved');

	const saved = await json(await request.get(`${API}/blogs`, { headers: account.headers }));
	const appearance = saved.find((blog) => blog.id === account.blog.id).embed_appearance;
	expect(appearance.typography.body_size).toBe(110);
	expect(appearance.colors.text).toBe('#123456');
	expect(appearance.typography.font).toBeUndefined();
	expect(appearance.typography.google_font).toBeUndefined();
	await expect(section.locator('select')).toHaveCount(0);

	// The preview renders through the embed's own stylesheet inside a shadow
	// root: the saved tokens are visible there immediately.
	const previewTitle = section.locator('.preview-host .statesu-title');
	await expect(previewTitle).toHaveText('Sample post');
	await expect.poll(() => previewTitle.evaluate((el) => getComputedStyle(el).color)).toBe('rgb(18, 52, 86)');
	await expect(section.locator('.preview-host .statesu-body h2')).toHaveText('Section heading');
	const previewImage = section.locator('.preview-host .statesu-body img');
	await previewImage.scrollIntoViewIfNeeded();
	await expect.poll(() => previewImage.evaluate((image) => image.complete && image.naturalWidth > 0)).toBe(true);

	// The feed sample renders announcement entries with the divider.
	await section.getByRole('button', { name: 'Feed', exact: true }).click();
	await expect(section.locator('.preview-host .statesu-announcement')).toHaveCount(2);
	await section.getByRole('button', { name: 'Post', exact: true }).click();
	await expect(section.locator('.preview-host .statesu-body img')).toHaveCount(1);

	// Discard returns to the saved state; unsetting everything clears the
	// saved appearance via the Default preset.
	await section.getByRole('button', { name: 'Default', exact: true }).click();
	await section.getByRole('button', { name: 'Save appearance', exact: true }).click();
	await expect(section.getByRole('status')).toContainText('Appearance saved');
	const cleared = await json(await request.get(`${API}/blogs`, { headers: account.headers }));
	expect(cleared.find((blog) => blog.id === account.blog.id).embed_appearance).toBeUndefined();
});

test('inherited typography stays clean after opening and discarding edits', async ({ page, request, account }) => {
	await signIn(page, account);
	for (const typography of [{}, { body_size: 110 }]) {
		await json(await request.put(`${API}/blogs/${account.blog.id}/appearance`, {
			headers: account.headers,
			data: { embed_appearance: { version: 1, typography } }
		}));
		await page.goto(`${API}/dashboard`);
		await page.getByRole('button', { name: 'Settings for Browser blog', exact: true }).click();
		const section = page.locator('.appearance');
		const save = section.getByRole('button', { name: 'Save appearance', exact: true });
		const discard = section.getByRole('button', { name: 'Discard changes', exact: true });
		await expect(save).toBeDisabled();
		await expect(discard).toBeDisabled();
		const size = section.getByLabel('Body size (%)', { exact: true });
		await size.fill('125');
		await expect(save).toBeEnabled();
		await discard.click();
		await expect(size).toHaveValue(typography.body_size ? '110' : '');
		await expect(save).toBeDisabled();
		await expect(discard).toBeDisabled();
		await expect(section.getByText('Unsaved changes', { exact: true })).toHaveCount(0);
	}
});

test('unconfigured v2 lists keep native nested spacing and configured links cover both states', async ({ page, request, account }) => {
	await createPost(request, account, 'nested', {
		status: 'published',
		content_markdown: '- Parent\n  - Child\n\n1. Parent\n   1. Child\n\n[Link](https://example.com)'
	});
	await page.goto(postUrl(account, 'nested', { v: '2' }));
	const nested = page.locator('.statesu-body ul ul, .statesu-body ol ol');
	await expect(nested).toHaveCount(2);
	for (const list of await nested.all()) {
		await expect(list).toHaveCSS('margin-block-start', '0px');
		await expect(list).toHaveCSS('margin-block-end', '0px');
	}
	const link = page.locator('.statesu-body a');
	await expect(link).toHaveCSS('color', 'rgb(0, 0, 238)');
	// Browser privacy deliberately hides visited colors from computed style.
	// Check the actual rule covers :any-link (both :link and :visited), then
	// verify that rule's saved color and host override in the real bundle.
	const selector = await link.evaluate((element) => {
		const sheet = element.getRootNode().querySelector('style').sheet;
		return [...sheet.cssRules].find((rule) => rule.style?.color.includes('--statesu-link-color'))?.selectorText;
	});
	expect(selector).toBe('.statesu-v2 .statesu-body a:any-link');
	await json(await request.put(`${API}/blogs/${account.blog.id}/appearance`, {
		headers: account.headers,
		data: { embed_appearance: { version: 1, colors: { link: '#123456' }, layout: { spacing: 24 } } }
	}));
	await page.reload();
	await expect(link).toHaveCSS('color', 'rgb(18, 52, 86)');
	for (const list of await nested.all()) await expect(list).toHaveCSS('margin-block-start', '24px');
	await page.goto(postUrl(account, 'nested', { v: '2', style: '#statesu-post { --statesu-link-color: #00ff00; }' }));
	await expect(link).toHaveCSS('color', 'rgb(0, 255, 0)');
});

test('the dashboard preview and the real embed share one computed style', async ({ page, request, account }) => {
	await json(await request.put(`${API}/blogs/${account.blog.id}/appearance`, {
		headers: account.headers,
		data: {
			embed_appearance: {
				version: 1,
				colors: { text: '#1a2b3c', link: '#7c3aed', muted: '#6e6e6e', background: '#ffffff' },
				typography: { body_size: 110, heading_scale: 120, line_height: 180 },
				layout: { max_width: 720, padding: 24, spacing: 16 },
				decoration: { border_width: 1, corner_radius: 8 }
			}
		}
	}));
	// The created post mirrors the editor's sample structures exactly, so
	// both surfaces render the same elements under the same tokens.
	await createPost(request, account, 'parity', {
		status: 'published',
		content_markdown:
			'A paragraph with a [link](https://example.com), some **bold** and *emphasis* text.\n\n' +
			'## Section heading\n\n' +
			'- First list item\n- Second list item\n\n' +
			'> A quoted line of text\n\n' +
			'```js\nconst ready = true;\n```\n\n' +
			`![Sample image](${API}/embed-preview.svg)`
	});

	const selectors = [
		'.statesu-title', '.statesu-body p', '.statesu-body ul', '.statesu-body blockquote',
		'.statesu-body pre', '.statesu-body a', '.statesu-body img'
	];
	const properties = ['color', 'fontFamily', 'fontSize', 'lineHeight', 'marginTop', 'marginBottom', 'paddingTop', 'maxWidth', 'borderRadius'];
	async function capture(locator) {
		return locator.evaluate((host, { selectors, properties }) => {
			const read = (element) => {
				const style = getComputedStyle(element);
				return Object.fromEntries(properties.map((name) => [name, style[name]]));
			};
			return Object.fromEntries(selectors.map((selector) => {
				const element = host.shadowRoot.querySelector(selector);
				return [selector, element ? read(element) : null];
			}));
		}, { selectors, properties });
	}

	await signIn(page, account);
	await page.getByRole('button', { name: 'Settings for Browser blog', exact: true }).click();
	// The preview shadow tree is built in an effect after mount; wait for
	// its content before measuring.
	await expect(page.locator('.preview-host .statesu-title')).toHaveText('Sample post');
	// Family is now inherited, so compare the two surfaces under the same host CSS.
	await page.locator('.preview-host').evaluate((host) => { host.style.fontFamily = 'Georgia, serif'; });
	const previewStyles = await capture(page.locator('.preview-host'));

	await page.goto(postUrl(account, 'parity', { v: '2', style: '#statesu-post { font-family: Georgia, serif; }' }));
	// The shadow root lives on the embed's host child, not on the target.
	await expect(page.locator('.statesu-title')).toHaveText('Post parity');
	const embedStyles2 = await capture(page.locator('[data-statesu-v2-host]'));
	expect(embedStyles2).toEqual(previewStyles);

	// Shadow isolation: a host rule targeting the embed's internals cannot
	// pierce the shadow root, while a variable on the target still inherits.
	await page.goto(postUrl(account, 'parity', {
		v: '2',
		style: '#statesu-post { --statesu-text-color: #00aa00; } #statesu-post p { color: #ff00ff !important; }'
	}));
	const paragraph = page.locator('.statesu-body p').first();
	await expect(paragraph).toHaveText(/A paragraph/);
	await expect.poll(() => paragraph.evaluate((el) => getComputedStyle(el).color)).toBe('rgb(0, 170, 0)');
});

test('the themed embed holds together on narrow viewports', async ({ page, request, account }) => {
	await json(await request.put(`${API}/blogs/${account.blog.id}/appearance`, {
		headers: account.headers,
		data: {
			embed_appearance: {
				version: 1,
				layout: { max_width: 720, padding: 24, spacing: 16 },
				decoration: { border_width: 1, corner_radius: 8 }
			}
		}
	}));
	await createPost(request, account, 'narrow', {
		status: 'published',
		content_markdown: `![Sample image](${API}/embed-preview.svg)\n\n- First list item\n- Second list item`
	});
	await page.setViewportSize({ width: 360, height: 720 });
	await page.goto(postUrl(account, 'narrow', { v: '2' }));
	const frame = page.locator('.statesu-v2');
	await expect(frame.locator('img')).toHaveCount(1);
	await expect.poll(() => frame.evaluate((element) => ({
		scroll: element.scrollWidth,
		client: element.clientWidth,
		padding: getComputedStyle(element).paddingLeft
	}))).toEqual({ scroll: expect.any(Number), client: expect.any(Number), padding: '24px' });
	const overflow = await frame.evaluate((element) => element.scrollWidth - element.clientWidth);
	expect(overflow).toBeLessThanOrEqual(1);
	const image = frame.locator('img');
	await image.scrollIntoViewIfNeeded();
	await expect.poll(() => image.evaluate((img) => img.naturalWidth > 0)).toBe(true);
	await expect.poll(() => image.evaluate((img) => img.clientWidth <= img.closest('article').clientWidth)).toBe(true);
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
	// The media renderer makes dashboard previews legitimate user content:
	// hostile Markdown still yields zero script-bearing elements, and any
	// rendered image is sanitized by the shared allowlist contract.
	await expect(page.locator('main svg, main math, main iframe')).toHaveCount(0);
	const previewImages = page.locator('main img');
	const preview = await previewImages.evaluateAll((images) =>
		images.map((img) => ({
			handlers: [...img.attributes].some((attribute) => attribute.name.toLowerCase().startsWith('on')),
			loading: img.getAttribute('loading')
		})));
	for (const image of preview) {
		expect(image.handlers).toBe(false);
		expect(image.loading).toBe('lazy');
	}
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
	// Tracker requests now come exclusively from the media preview's
	// sanitized <img> elements: script, style, iframe and handler-driven
	// loaders never leave the page (window.__xss stays false above).
	expect(dialogs).toEqual([]);
	expect(trackers.every((url) => url === `${HOST}/tracker`)).toBe(true);
	expect(trackers.length).toBeGreaterThan(0);
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
	await expect(snippet).toContainText(`src="${API}/embed/v2.js"`);
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
