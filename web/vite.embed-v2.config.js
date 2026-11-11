import { defineConfig } from 'vite';

// Standalone IIFE build of the embed v2 script (shadow root, responsive
// media, lightbox). Output lands in `build/embed/` so the adapter-static
// output carries it verbatim into assets/dist next to v1.js.
export default defineConfig({
	build: {
		outDir: 'build/embed',
		emptyOutDir: false,
		target: 'es2019',
		lib: {
			entry: 'src/embed/main-v2.js',
			name: 'statesuEmbedV2',
			formats: ['iife'],
			fileName: () => 'v2.js'
		}
	}
});
