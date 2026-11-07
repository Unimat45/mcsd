<script lang="ts">
	import { api, type ServerInfo } from './api';

	let {
		server,
		onchange,
		size = 'md'
	}: { server: ServerInfo; onchange?: (s: ServerInfo) => void; size?: 'sm' | 'md' } = $props();

	let busy = $state<string | null>(null);
	let error = $state('');

	async function run(action: 'start' | 'stop' | 'restart' | 'kill') {
		if (action === 'kill' && !confirm(`Kill ${server.name}? The world will NOT be saved.`)) return;
		busy = action;
		error = '';
		try {
			const q = action === 'kill' ? '' : '?wait=false';
			const s = await api<ServerInfo>(`/servers/${server.name}/${action}${q}`, { method: 'POST' });
			onchange?.(s);
		} catch (e) {
			error = (e as Error).message;
		} finally {
			busy = null;
		}
	}

	const st = $derived(server.status);
	const canStart = $derived(st === 'stopped' || st === 'crashed' || st === 'error');
	const canStop = $derived(st === 'running' || st === 'starting' || st === 'crashed');
	const pad = $derived(size === 'sm' ? '!px-2 !py-1 !text-xs' : '');
</script>

<div class="flex flex-col items-end gap-1">
	<div class="flex flex-wrap gap-2">
		{#if canStart}
			<button class="btn btn-primary {pad}" disabled={!!busy} onclick={() => run('start')}>
				{busy === 'start' ? 'Starting…' : 'Start'}
			</button>
		{/if}
		{#if canStop}
			<button class="btn btn-ghost {pad}" disabled={!!busy} onclick={() => run('stop')}>
				{busy === 'stop' ? 'Stopping…' : 'Stop'}
			</button>
		{/if}
		{#if st === 'running'}
			<button class="btn btn-ghost {pad}" disabled={!!busy} onclick={() => run('restart')}>Restart</button>
		{/if}
		{#if st === 'stopping' || st === 'starting'}
			<button class="btn btn-danger {pad}" disabled={busy === 'kill'} onclick={() => run('kill')}>Kill</button>
		{/if}
	</div>
	{#if error}<p class="max-w-xs text-right text-xs text-red-400">{error}</p>{/if}
</div>
