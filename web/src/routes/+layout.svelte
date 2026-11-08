<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { auth, captureTokenFromHash } from '$lib/api';

	let { children } = $props();
	let ready = $state(false);

	onMount(() => {
		captureTokenFromHash();
		if (!auth.token && page.url.pathname !== '/login') goto('/login');
		ready = true;
	});

	function logout() {
		auth.clear();
		goto('/login');
	}

	const nav = [
		{ href: '/', label: 'Servers' },
		{ href: '/java', label: 'Java' }
	];
	const isActive = (href: string) =>
		href === '/' ? page.url.pathname === '/' || page.url.pathname.startsWith('/servers') : page.url.pathname.startsWith(href);
</script>

<div class="min-h-screen">
	<header class="sticky top-0 z-10 border-b border-zinc-800 bg-zinc-950/80 backdrop-blur">
		<div class="mx-auto flex h-14 max-w-6xl items-center gap-8 px-4">
			<a href="/" class="flex items-center gap-2 font-semibold tracking-tight">
				<span class="grass"></span> mcsd
			</a>
			{#if page.url.pathname !== '/login'}
				<nav class="flex gap-5 text-sm">
					{#each nav as item}
						<a href={item.href} class={isActive(item.href) ? 'text-white' : 'text-zinc-400 hover:text-zinc-200'}>{item.label}</a>
					{/each}
				</nav>
				<button class="ml-auto text-sm text-zinc-400 hover:text-zinc-200" onclick={logout}>Log out</button>
			{/if}
		</div>
	</header>
	<main class="mx-auto max-w-6xl px-4 py-8">
		{#if ready}{@render children()}{/if}
	</main>
</div>
