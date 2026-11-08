<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type JavaRuntime, type SystemInfo } from '$lib/api';

	let runtimes = $state<JavaRuntime[] | null>(null);
	let system = $state<SystemInfo | null>(null);
	let major = $state(21);
	let installing = $state(false);
	let message = $state('');

	async function load(refresh = false) {
		runtimes = await api<JavaRuntime[]>(`/java${refresh ? '?refresh=true' : ''}`);
	}

	onMount(() => {
		load();
		api<SystemInfo>('/system').then((s) => (system = s));
	});

	async function install() {
		installing = true;
		message = `Downloading Java ${major} from Adoptium…`;
		try {
			const rt = await api<JavaRuntime>('/java/install', { method: 'POST', body: { major } });
			message = `Installed Java ${rt.version}.`;
			await load(true);
		} catch (e) {
			message = `Error: ${(e as Error).message}`;
		} finally {
			installing = false;
		}
	}
</script>

<div class="mb-6 flex items-end justify-between">
	<div>
		<h1 class="text-2xl font-semibold tracking-tight">Java runtimes</h1>
		<p class="mt-1 text-sm text-zinc-400">
			Each server picks the Java version its Minecraft version needs. Missing runtimes are downloaded automatically.
		</p>
	</div>
	<button class="btn btn-ghost" onclick={() => load(true)}>Rescan</button>
</div>

<div class="card overflow-hidden">
	<table class="w-full text-sm">
		<thead class="border-b border-zinc-800 text-left text-xs text-zinc-500 uppercase">
			<tr><th class="px-4 py-3">Version</th><th class="px-4 py-3">Path</th><th class="px-4 py-3">Source</th></tr>
		</thead>
		<tbody>
			{#if runtimes === null}
				<tr><td class="px-4 py-3 text-zinc-500" colspan="3">Scanning…</td></tr>
			{:else if runtimes.length === 0}
				<tr><td class="px-4 py-3 text-zinc-500" colspan="3">No Java found. Install one below.</td></tr>
			{/if}
			{#each runtimes ?? [] as r}
				<tr class="border-b border-zinc-800/60 last:border-0">
					<td class="px-4 py-3"><span class="font-semibold">{r.major}</span> <span class="text-zinc-500">({r.version})</span></td>
					<td class="px-4 py-3 font-mono text-xs break-all text-zinc-300">{r.path}</td>
					<td class="px-4 py-3 text-zinc-400">{r.managed ? 'mcsd' : 'system'}</td>
				</tr>
			{/each}
		</tbody>
	</table>
</div>

<div class="card mt-6 flex flex-wrap items-end gap-4 p-6">
	<div>
		<label class="label" for="major">Install Temurin</label>
		<select id="major" class="input w-40" bind:value={major}>
			{#each [25, 21, 17, 11, 8] as m}<option value={m}>Java {m}</option>{/each}
		</select>
	</div>
	<button class="btn btn-primary" disabled={installing} onclick={install}>{installing ? 'Installing…' : 'Install'}</button>
	{#if message}<p class="text-sm {message.startsWith('Error') ? 'text-red-400' : 'text-zinc-400'}">{message}</p>{/if}
</div>

{#if system}
	<p class="mt-6 text-xs text-zinc-500">
		mcsd {system.version} · {system.os}/{system.arch} · {system.cpus} CPUs · data in <span class="font-mono">{system.data_dir}</span>
	</p>
{/if}
