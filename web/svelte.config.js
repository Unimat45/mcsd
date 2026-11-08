import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
export default {
	preprocess: vitePreprocess(),
	kit: {
		// Pure SPA: the daemon serves index.html for every non-API route.
		adapter: adapter({ fallback: 'index.html' })
	}
};
