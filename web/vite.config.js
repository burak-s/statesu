import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { loadEnv, defineConfig } from 'vite';

// Proxy API requests during local documentation development.
const apiTarget = process.env.STATESU_API ?? 'http://localhost:8080';

// The dashboard is a prerendered static site, so SvelteKit's env channels
// cannot hand browser code a runtime value: the Turnstile site key is baked
// in here from the build .env (see .env.example) — empty disables the widget.
const turnstileSiteKey = (mode) =>
	loadEnv(mode, '.', '')[ 'PUBLIC_TURNSTILE_SITE_KEY' ] ??
	process.env.PUBLIC_TURNSTILE_SITE_KEY ??
	'';

export default defineConfig(({ mode }) => ({
	plugins: [
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},

			// The homepage is prerendered as static documentation.
			adapter: adapter()
		})
	],
	define: {
		'import.meta.env.PUBLIC_TURNSTILE_SITE_KEY': JSON.stringify(turnstileSiteKey(mode))
	},
	server: {
		proxy: {
			'/auth': apiTarget,
			'/blogs': apiTarget,
			'/public': apiTarget
		}
	}
}));
