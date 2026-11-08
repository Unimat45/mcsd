<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { api, fmtBytes, type Kind, type ServerInfo, type SystemInfo } from '$lib/api';

	let name = $state('');
	let kind = $state<Kind>('paper');
	let version = $state('');
	let versions = $state<string[]>([]);
	let loadingVersions = $state(false);
	let memory = $state(2048);
	let port = $state<number | null>(null);
	let autostart = $state(true);
	let startNow = $state(true);
	let eula = $state(false);
	let jarSource = $state('');
	let javaMajor = $state<number | null>(null);
	let error = $state('');
	let submitting = $state(false);
	let system = $state<SystemInfo | null>(null);

	const kinds: { id: Kind; label: string; desc: string }[] = [
		{ id: 'paper', label: 'Paper', desc: 'Fast, plugin support (recommended)' },
		{ id: 'vanilla', label: 'Vanilla', desc: 'Official Mojang server' },
		{ id: 'fabric', label: 'Fabric', desc: 'Lightweight mod loader' },
		{ id: 'custom', label: 'Custom', desc: 'Bring your own jar (Forge, Purpur…)' }
	];

	onMount(async () => {
		system = await api<SystemInfo>('/system').catch(() => null);
	});

	$effect(() => {
		const k = kind;
		versions = [];
		version = '';
		if (k === 'custom') return;
		loadingVersions = true;
		api<string[]>(`/versions/${k}`)
			.then((v) => {
				if (kind !== k) return;
				versions = v;
				version = v[0] ?? '';
			})
			.catch((e) => (error = `Could not load versions: ${e.message}`))
			.finally(() => (loadingVersions = false));
	});

	const nameValid = $derived(/^[a-z0-9][a-z0-9_-]{0,31}$/.test(name));
	const maxMem = $derived(system ? Math.floor(system.total_memory_bytes / 1024 / 1024 / 512) * 512 : 16384);

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		error = '';
		submitting = true;
		try {
			const s = await api<ServerInfo>('/servers', {
				method: 'POST',
				body: {
					name,
					kind,
					version: kind === 'custom' ? version || 'custom' : version,
					memory_mb: memory,
					port: port || undefined,
					accept_eula: eula,
					autostart,
					start: startNow,
					jar_source: kind === 'custom' && jarSource ? jarSource : undefined,
					java_major: javaMajor || undefined
				}
			});
			goto(`/servers/${s.name}`);
		} catch (err) {
			error = (err as Error).message;
		} finally {
			submitting = false;
		}
	}
</script>

<div class="mx-auto max-w-2xl">
	<a href="/" class="text-sm text-zinc-400 hover:text-zinc-200">← Servers</a>
	<h1 class="mt-2 mb-6 text-2xl font-semibold tracking-tight">New server</h1>

	<form class="card space-y-6 p-6" onsubmit={submit}>
		<div>
			<label class="label" for="name">Name</label>
			<input id="name" class="input" bind:value={name} placeholder="survival" autocomplete="off" />
			{#if name && !nameValid}
				<p class="mt-1 text-xs text-red-400">Lowercase letters, digits, - and _ (max 32).</p>
			{/if}
		</div>

		<div>
			<span class="label">Type</span>
			<div class="grid grid-cols-2 gap-2 sm:grid-cols-4">
				{#each kinds as k}
					<button
						type="button"
						onclick={() => (kind = k.id)}
						class="rounded-lg border p-3 text-left transition-colors {kind === k.id
							? 'border-emerald-500 bg-emerald-500/10'
							: 'border-zinc-700 hover:border-zinc-500'}"
					>
						<div class="text-sm font-medium">{k.label}</div>
						<div class="mt-0.5 text-xs text-zinc-400">{k.desc}</div>
					</button>
				{/each}
			</div>
		</div>

		<div class="grid gap-4 sm:grid-cols-2">
			<div>
				<label class="label" for="version">Minecraft version</label>
				{#if kind === 'custom'}
					<input id="version" class="input" bind:value={version} placeholder="e.g. 1.21.1 (used to pick Java)" />
				{:else}
					<select id="version" class="input" bind:value={version} disabled={loadingVersions}>
						{#if loadingVersions}<option>Loading…</option>{/if}
						{#each versions as v}<option value={v}>{v}</option>{/each}
					</select>
				{/if}
			</div>
			<div>
				<label class="label" for="port">Port</label>
				<input id="port" class="input" type="number" min="1024" max="65535" bind:value={port} placeholder="auto (25565+)" />
			</div>
		</div>

		<div>
			<label class="label" for="memory">Memory: {memory} MB</label>
			<input id="memory" type="range" class="w-full accent-emerald-500" min="512" max={maxMem} step="256" bind:value={memory} />
			{#if system}
				<p class="mt-1 text-xs text-zinc-500">
					Host has {fmtBytes(system.total_memory_bytes)} total, {fmtBytes(system.available_memory_bytes)} available.
				</p>
			{/if}
		</div>

		{#if kind === 'custom'}
			<div class="grid gap-4 sm:grid-cols-2">
				<div>
					<label class="label" for="jar">Jar path (on the daemon host)</label>
					<input id="jar" class="input font-mono" bind:value={jarSource} placeholder="/path/to/server.jar" />
					<p class="mt-1 text-xs text-zinc-500">Or leave empty and drop <code>server.jar</code> in the server folder.</p>
				</div>
				<div>
					<label class="label" for="java">Java version</label>
					<input id="java" class="input" type="number" bind:value={javaMajor} placeholder="auto" />
				</div>
			</div>
		{/if}

		<div class="space-y-2 text-sm">
			<label class="flex items-center gap-2"><input type="checkbox" class="accent-emerald-500" bind:checked={startNow} /> Start after install</label>
			<label class="flex items-center gap-2"><input type="checkbox" class="accent-emerald-500" bind:checked={autostart} /> Start automatically with the daemon</label>
			<label class="flex items-start gap-2 rounded-lg border border-zinc-800 bg-zinc-950 p-3">
				<input type="checkbox" class="mt-0.5 accent-emerald-500" bind:checked={eula} />
				<span>I agree to the <a class="text-emerald-400 underline" href="https://aka.ms/MinecraftEULA" target="_blank" rel="noreferrer">Minecraft EULA</a>.</span>
			</label>
		</div>

		{#if error}<p class="text-sm text-red-400">{error}</p>{/if}

		<div class="flex justify-end gap-2">
			<a href="/" class="btn btn-ghost">Cancel</a>
			<button class="btn btn-primary" disabled={submitting || !nameValid || !eula || (kind !== 'custom' && !version)}>
				{submitting ? 'Creating…' : 'Create server'}
			</button>
		</div>
	</form>
</div>
