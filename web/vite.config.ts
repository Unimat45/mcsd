import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

// `npm run dev` proxies the API to a running daemon.
const daemon = process.env.MCSD_URL ?? 'http://127.0.0.1:8765';

export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],
	server: {
		proxy: {
			'/api': { target: daemon, ws: true, changeOrigin: true }
		}
	}
});
