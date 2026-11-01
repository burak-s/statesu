// Build the release assets and serve the real Rust router on one origin,
// plus a consumer site's deep-link template on a second, cross-site origin.
import { execFileSync, spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const root = fileURLToPath(new URL('../../', import.meta.url));
const apiPort = process.env.STATESU_BROWSER_API_PORT ?? '4180';
const hostPort = process.env.STATESU_BROWSER_HOST_PORT ?? '4181';
const api = `http://localhost:${apiPort}`;

// The test backend disables Turnstile; keep the bundled login form aligned,
// independently of a developer's local production site key.
execFileSync('make', ['web'], {
	cwd: root,
	stdio: 'inherit',
	env: { ...process.env, PUBLIC_TURNSTILE_SITE_KEY: '' }
});
execFileSync('cargo', ['build', '--locked', '--example', 'browser_server', '--bin', 'stscli', '--bin', 'statesu-cli'], {
	cwd: root,
	stdio: 'inherit'
});
const target = resolve(root, process.env.CARGO_TARGET_DIR ?? 'target');
const backend = spawn(resolve(target, 'debug/examples/browser_server'), [], {
	cwd: root,
	stdio: 'inherit',
	env: { ...process.env, STATESU_BROWSER_API_PORT: apiPort }
});

function escape(value) {
	return value.replace(/[&<>"']/g, (char) => ({
		'&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
	})[char]);
}

const host = createServer(async (request, response) => {
	const url = new URL(request.url, `http://127.0.0.1:${hostPort}`);
	if (url.pathname === '/__test/ready') {
		try {
			const ready = await fetch(`http://127.0.0.1:${apiPort}/__test/ready`);
			response.writeHead(ready.ok ? 200 : 503).end('ready');
		} catch {
			response.writeHead(503).end('starting');
		}
		return;
	}
	if (url.pathname === '/favicon.ico') {
		response.writeHead(204).end();
		return;
	}
	const blog = escape(url.searchParams.get('blog') ?? 'browser');
	const base = escape(url.searchParams.get('base') ?? '/blog');
	const targetId = escape(url.searchParams.get('target') ?? 'statesu-post');
	response.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
	response.end(`<!doctype html><html><head><meta charset="utf-8"><title>Consumer site</title></head>
<body><header id="host-header"><a href="/outside">Host navigation</a></header>
<div id="statesu-post">Host placeholder</div><footer id="host-footer">Host footer</footer>
${url.searchParams.has('unsupported') ? '<script>DOMImplementation.prototype.createHTMLDocument = undefined;</script>' : ''}
<script defer src="${api}/embed/v1.js" data-blog="${blog}" data-base-path="${base}" data-target="${targetId}"></script>
</body></html>`);
});

let stopping = false;
function stop() {
	if (stopping) return;
	stopping = true;
	host.closeAllConnections();
	host.close();
	backend.kill('SIGINT');
}
process.on('SIGINT', stop);
process.on('SIGTERM', stop);
backend.on('error', (error) => {
	console.error(error);
	process.exitCode = 1;
	stop();
});
backend.on('exit', (code) => {
	if (!stopping) {
		console.error(`Browser-test API exited unexpectedly (${code}).`);
		process.exitCode = 1;
		stop();
	}
});
host.on('error', (error) => {
	console.error(error);
	process.exitCode = 1;
	stop();
});
host.listen(Number(hostPort), '127.0.0.1');
