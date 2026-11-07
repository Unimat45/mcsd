<script lang="ts">
	import { onMount } from 'svelte';
	import { api, fmtBytes, fmtUptime, isActive, type ServerInfo } from '$lib/api';
	import StatusBadge from '$lib/StatusBadge.svelte';
	import ServerActions from '$lib/ServerActions.svelte';

	let servers = $state<ServerInfo[] | null>(null);
	let error = $state('');

	async function load() {
		try {
			servers = await api<ServerInfo[]>('/servers');
			error = '';
		} catch (e) {
			error = (e as Error).message;
		}
	}

	onMount(() => {
		load();
		const t = setInterval(load, 2000);
		return () => clearInterval(t);
	});

	function replace(s: ServerInfo) {
		if (servers) servers = servers.map((x) => (x.name === s.name ? s : x));
	}

	const memPct = (s: ServerInfo) => Math.min(100, (s.memory_bytes / (s.memory_mb * 1024 * 1024)) * 100);
	const running = $derived(servers?.filter((s) => s.status === 'running').length ?? 0);
	const players = $derived(servers?.reduce((n, s) => n + s.players.length, 0) ?? 0);
</script>

<div class="mb-8 flex items-end justify-between gap-4">
	<div>
		<h1 class="text-2xl font-semibold tracking-tight">Servers</h1>
		{#if servers}
			<p class="mt-1 text-sm text-zinc-400">
				{servers.length} registered · {running} running · {players} player{players === 1 ? '' : 's'} online
			</p>
		{/if}
	</div>
	<a href="/servers/new" class="btn btn-primary">+ New server</a>
</div>

{#if error}
	<div class="card mb-6 border-red-900 p-4 text-sm text-red-300">Cannot reach the daemon: {error}</div>
{/if}

{#if servers === null}
	<p class="text-zinc-500">Loading…</p>
{:else if servers.length === 0}
	<div class="card flex flex-col items-center gap-4 p-16 text-center">
		<span class="grass scale-[2.5]"></span>
		<h2 class="mt-4 text-lg font-medium">No servers yet</h2>
		<p class="max-w-sm text-sm text-zinc-400">
			Create your first server: mcsd downloads the jar, finds or installs the right Java and accepts the EULA for you.
		</p>
		<a href="/servers/new" class="btn btn-primary">Create a server</a>
	</div>
{:else}
	<div class="grid gap-4 md:grid-cols-2">
		{#each servers as s (s.name)}
			<div class="card flex flex-col gap-4 p-5 transition-colors hover:border-zinc-700">
				<div class="flex items-start justify-between gap-3">
					<a href="/servers/{s.name}" class="min-w-0">
						<h2 class="truncate text-lg font-semibold hover:text-emerald-300">{s.name}</h2>
						<p class="text-sm text-zinc-400 capitalize">{s.kind} {s.version} · :{s.port}</p>
					</a>
					<StatusBadge status={s.status} />
				</div>

				{#if s.status === 'installing' && s.progress}
					<p class="truncate text-xs text-violet-300">{s.progress}</p>
				{:else if (s.status === 'error' || s.status === 'crashed') && s.last_error}
					<p class="truncate text-xs text-red-400" title={s.last_error}>{s.last_error}</p>
				{/if}

				<dl class="grid grid-cols-4 gap-3 text-sm">
					<div>
						<dt class="text-xs text-zinc-500">Players</dt>
						<dd class="font-medium" title={s.players.join(', ')}>{isActive(s.status) ? s.players.length : '–'}</dd>
					</div>
					<div>
						<dt class="text-xs text-zinc-500">CPU</dt>
						<dd class="font-medium">{isActive(s.status) ? `${s.cpu_percent.toFixed(0)}%` : '–'}</dd>
					</div>
					<div>
						<dt class="text-xs text-zinc-500">Uptime</dt>
						<dd class="font-medium">{fmtUptime(s.uptime_secs)}</dd>
					</div>
					<div>
						<dt class="text-xs text-zinc-500">Memory</dt>
						<dd class="font-medium">{fmtBytes(s.memory_bytes)}</dd>
					</div>
				</dl>
				<div class="h-1 overflow-hidden rounded-full bg-zinc-800" title="Process memory vs. {s.memory_mb} MB heap">
					<div class="h-full bg-emerald-500 transition-all" style="width: {memPct(s)}%"></div>
				</div>

				<div class="flex items-center justify-between">
					<a href="/servers/{s.name}" class="text-sm text-zinc-400 hover:text-zinc-200">Console →</a>
					<ServerActions server={s} onchange={replace} size="sm" />
				</div>
			</div>
		{/each}
	</div>
{/if}
