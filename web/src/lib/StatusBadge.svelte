<script lang="ts">
	import type { Status } from './api';

	let { status }: { status: Status } = $props();

	const styles: Record<Status, string> = {
		running: 'bg-emerald-500/15 text-emerald-300 ring-emerald-500/30',
		starting: 'bg-sky-500/15 text-sky-300 ring-sky-500/30',
		stopping: 'bg-amber-500/15 text-amber-300 ring-amber-500/30',
		installing: 'bg-violet-500/15 text-violet-300 ring-violet-500/30',
		stopped: 'bg-zinc-500/15 text-zinc-300 ring-zinc-500/30',
		crashed: 'bg-red-500/15 text-red-300 ring-red-500/30',
		error: 'bg-red-500/15 text-red-300 ring-red-500/30'
	};
	const dot: Record<Status, string> = {
		running: 'bg-emerald-400',
		starting: 'bg-sky-400 animate-pulse',
		stopping: 'bg-amber-400 animate-pulse',
		installing: 'bg-violet-400 animate-pulse',
		stopped: 'bg-zinc-500',
		crashed: 'bg-red-400',
		error: 'bg-red-400'
	};
</script>

<span class="inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-xs font-medium ring-1 ring-inset {styles[status]}">
	<span class="h-1.5 w-1.5 rounded-full {dot[status]}"></span>
	{status}
</span>
