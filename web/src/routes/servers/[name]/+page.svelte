<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, fmtBytes, fmtUptime, isActive, type ServerInfo } from '$lib/api';
	import Console from '$lib/Console.svelte';
	import ServerActions from '$lib/ServerActions.svelte';
	import StatusBadge from '$lib/StatusBadge.svelte';

	const name = $derived(page.params.name ?? '');
	let server = $state<ServerInfo | null>(null);
	let notFound = $state(false);

	// settings form
	let form = $state({
		memory_mb: 0,
		port: 0,
		version: '',
		autostart: false,
		auto_restart: true,
		stop_timeout_secs: 60,
		jvm_args: '',
		java_path: ''
	});
	let formLoadedFor = '';
	let versions = $state<string[]>([]);
	let saving = $state(false);
	let saveMsg = $state('');
	let deleteFiles = $state(false);
	let dangerError = $state('');

	async function load() {
		try {
			server = await api<ServerInfo>(`/servers/${name}`);
			notFound = false;
			if (formLoadedFor !== name) {
				formLoadedFor = name;
				resetForm(server);
				if (server.kind !== 'custom')
					api<string[]>(`/versions/${server.kind}`)
						.then((v) => (versions = v))
						.catch(() => {});
			}
		} catch (e) {
			if ((e as { status?: number }).status === 404) notFound = true;
		}
	}

	function resetForm(s: ServerInfo) {
		form = {
			memory_mb: s.memory_mb,
			port: s.port,
			version: s.version,
			autostart: s.autostart,
			auto_restart: s.auto_restart,
			stop_timeout_secs: s.stop_timeout_secs,
			jvm_args: s.jvm_args.join('\n'),
			java_path: s.java_path ?? ''
		};
	}

	$effect(() => {
		name; // re-run when navigating between servers
		formLoadedFor = '';
		load();
		const t = setInterval(load, 2000);
		return () => clearInterval(t);
	});

	async function save(e: SubmitEvent) {
		e.preventDefault();
		saving = true;
		saveMsg = '';
		try {
			server = await api<ServerInfo>(`/servers/${name}`, {
				method: 'PATCH',
				body: {
					...form,
					jvm_args: form.jvm_args.split('\n').map((s) => s.trim()).filter(Boolean)
				}
			});
			resetForm(server);
			saveMsg = isActive(server.status) ? 'Saved — applies on next restart.' : 'Saved.';
		} catch (err) {
			saveMsg = `Error: ${(err as Error).message}`;
		} finally {
			saving = false;
		}
	}

	async function reinstall() {
		if (!confirm(`Re-download the server jar for ${name}? World data is kept.`)) return;
		dangerError = '';
		try {
			server = await api<ServerInfo>(`/servers/${name}/reinstall`, { method: 'POST' });
		} catch (err) {
			dangerError = (err as Error).message;
		}
	}

	async function remove() {
		const msg = deleteFiles
			? `Delete ${name} AND all its files (world included)? This cannot be undone.`
			: `Remove ${name} from mcsd? Files are kept on disk.`;
		if (!confirm(msg)) return;
		dangerError = '';
		try {
			await api(`/servers/${name}?delete_files=${deleteFiles}`, { method: 'DELETE' });
			goto('/');
		} catch (err) {
			dangerError = (err as Error).message;
		}
	}

	const stopped = $derived(server ? !isActive(server.status) && server.status !== 'installing' : false);
</script>

<a href="/" class="text-sm text-zinc-400 hover:text-zinc-200">← Servers</a>

{#if notFound}
	<p class="mt-6 text-zinc-400">No server named “{name}”.</p>
{:else if !server}
	<p class="mt-6 text-zinc-500">Loading…</p>
{:else}
	<div class="mt-2 mb-6 flex flex-wrap items-start justify-between gap-4">
		<div>
			<div class="flex items-center gap-3">
				<h1 class="text-2xl font-semibold tracking-tight">{server.name}</h1>
				<StatusBadge status={server.status} />
			</div>
			<p class="mt-1 text-sm text-zinc-400 capitalize">
				{server.kind} {server.version} · port {server.port} · Java {server.java_major ?? '?'}
			</p>
		</div>
		<ServerActions {server} onchange={(s) => (server = s)} />
	</div>

	{#if server.status === 'installing'}
		<div class="card mb-4 border-violet-900 p-4 text-sm text-violet-200">Installing… {server.progress ?? ''}</div>
	{/if}
	{#if server.last_error && (server.status === 'error' || server.status === 'crashed')}
		<div class="card mb-4 border-red-900 p-4 text-sm text-red-300">{server.last_error}</div>
	{/if}

	<div class="mb-6 grid grid-cols-2 gap-3 sm:grid-cols-5">
		{#each [{ k: 'Players', v: isActive(server.status) ? String(server.players.length) : '–' }, { k: 'CPU', v: isActive(server.status) ? `${server.cpu_percent.toFixed(0)}%` : '–' }, { k: 'Memory', v: `${fmtBytes(server.memory_bytes)} / ${server.memory_mb} MB` }, { k: 'Uptime', v: fmtUptime(server.uptime_secs) }, { k: 'Restarts', v: String(server.restarts) }] as stat}
			<div class="card px-4 py-3">
				<div class="text-xs text-zinc-500">{stat.k}</div>
				<div class="mt-0.5 font-medium">{stat.v}</div>
			</div>
		{/each}
	</div>

	{#if server.players.length}
		<div class="mb-6 flex flex-wrap gap-2">
			{#each server.players as p}
				<span class="flex items-center gap-2 rounded-full border border-zinc-800 bg-zinc-900 py-1 pr-3 pl-1 text-sm">
					<img src="https://mc-heads.net/avatar/{p}/20" alt="" class="h-5 w-5 rounded-full" />{p}
				</span>
			{/each}
		</div>
	{/if}

	<Console name={server.name} active={isActive(server.status)} />

	<div class="mt-8 grid gap-6 lg:grid-cols-3">
		<form class="card space-y-4 p-6 lg:col-span-2" onsubmit={save}>
			<h2 class="font-semibold">Settings</h2>
			<div class="grid gap-4 sm:grid-cols-3">
				<div>
					<label class="label" for="mem">Memory (MB)</label>
					<input id="mem" class="input" type="number" min="512" step="256" bind:value={form.memory_mb} />
				</div>
				<div>
					<label class="label" for="port">Port</label>
					<input id="port" class="input" type="number" min="1024" max="65535" bind:value={form.port} />
				</div>
				<div>
					<label class="label" for="ver">Version</label>
					{#if server.kind === 'custom' || versions.length === 0}
						<input id="ver" class="input" bind:value={form.version} disabled={!stopped} />
					{:else}
						<select id="ver" class="input" bind:value={form.version} disabled={!stopped} title={stopped ? '' : 'Stop the server to change version'}>
							{#each versions as v}<option value={v}>{v}</option>{/each}
						</select>
					{/if}
				</div>
			</div>
			<div class="grid gap-4 sm:grid-cols-2">
				<div>
					<label class="label" for="jp">Java binary (empty = automatic)</label>
					<input id="jp" class="input font-mono" bind:value={form.java_path} placeholder="auto" />
				</div>
				<div>
					<label class="label" for="st">Stop timeout (seconds)</label>
					<input id="st" class="input" type="number" min="5" bind:value={form.stop_timeout_secs} />
				</div>
			</div>
			<div>
				<label class="label" for="jvm">Extra JVM arguments (one per line)</label>
				<textarea id="jvm" class="input h-24 font-mono" bind:value={form.jvm_args} placeholder="-XX:+UseG1GC"></textarea>
			</div>
			<div class="flex flex-wrap gap-6 text-sm">
				<label class="flex items-center gap-2"><input type="checkbox" class="accent-emerald-500" bind:checked={form.autostart} /> Start with daemon</label>
				<label class="flex items-center gap-2"><input type="checkbox" class="accent-emerald-500" bind:checked={form.auto_restart} /> Restart on crash</label>
			</div>
			<div class="flex items-center justify-end gap-3">
				{#if saveMsg}<span class="text-sm {saveMsg.startsWith('Error') ? 'text-red-400' : 'text-zinc-400'}">{saveMsg}</span>{/if}
				<button class="btn btn-primary" disabled={saving}>{saving ? 'Saving…' : 'Save settings'}</button>
			</div>
		</form>

		<div class="space-y-6">
			<div class="card space-y-2 p-6 text-sm">
				<h2 class="mb-2 font-semibold">Details</h2>
				<p class="text-zinc-400">Directory</p>
				<p class="font-mono text-xs break-all">{server.dir}</p>
				{#if server.pid}<p class="text-zinc-400">PID <span class="font-mono text-zinc-200">{server.pid}</span></p>{/if}
				{#if server.last_exit}<p class="text-zinc-400">Last exit: <span class="text-zinc-200">{server.last_exit}</span></p>{/if}
			</div>
			<div class="card space-y-4 border-red-950 p-6">
				<h2 class="font-semibold text-red-300">Danger zone</h2>
				<button class="btn btn-ghost w-full" disabled={!stopped || server.kind === 'custom'} onclick={reinstall}>
					Re-download server jar
				</button>
				<label class="flex items-center gap-2 text-sm">
					<input type="checkbox" class="accent-red-500" bind:checked={deleteFiles} /> Also delete world &amp; files
				</label>
				<button class="btn btn-danger w-full" disabled={!stopped} onclick={remove}>Delete server</button>
				{#if !stopped}<p class="text-xs text-zinc-500">Stop the server first.</p>{/if}
				{#if dangerError}<p class="text-xs text-red-400">{dangerError}</p>{/if}
			</div>
		</div>
	</div>
{/if}
