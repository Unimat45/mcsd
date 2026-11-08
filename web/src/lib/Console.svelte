<script lang="ts">
	import { tick } from 'svelte';
	import { consoleUrl, type LogLine } from './api';

	let { name, active }: { name: string; active: boolean } = $props();

	const MAX_LINES = 2000;
	let lines = $state.raw<LogLine[]>([]);
	let connected = $state(false);
	let input = $state('');
	let box: HTMLDivElement | undefined = $state();
	let follow = $state(true);
	let ws: WebSocket | null = null;
	const history: string[] = [];
	let historyIdx = -1;

	// A derived string only notifies when its *value* changes. Reading the prop
	// directly would subscribe to whatever object the parent built it from
	// (e.g. a polled `server` object) and reopen the socket on every poll.
	const serverName = $derived(name);

	// Connect (and reconnect) per server.
	$effect(() => {
		const server = serverName;
		let closed = false;
		let retry: ReturnType<typeof setTimeout>;
		let pending: LogLine[] = [];
		let frame = 0;

		const flush = async () => {
			frame = 0;
			lines = lines.concat(pending).slice(-MAX_LINES);
			pending = [];
			if (follow) {
				await tick();
				box?.scrollTo({ top: box.scrollHeight });
			}
		};

		const connect = () => {
			const sock = new WebSocket(consoleUrl(server));
			ws = sock;
			sock.onopen = () => {
				connected = true;
				lines = [];
			};
			sock.onmessage = (ev) => {
				pending.push(JSON.parse(ev.data));
				if (!frame) frame = requestAnimationFrame(flush);
			};
			sock.onclose = () => {
				connected = false;
				if (!closed) retry = setTimeout(connect, 2000);
			};
		};
		connect();

		return () => {
			closed = true;
			clearTimeout(retry);
			if (frame) cancelAnimationFrame(frame);
			ws?.close();
			ws = null;
		};
	});

	function onScroll() {
		if (!box) return;
		follow = box.scrollTop + box.clientHeight >= box.scrollHeight - 24;
	}

	function send(e: SubmitEvent) {
		e.preventDefault();
		const cmd = input.trim();
		if (!cmd || !ws || ws.readyState !== WebSocket.OPEN) return;
		ws.send(cmd);
		if (history[history.length - 1] !== cmd) history.push(cmd);
		historyIdx = -1;
		input = '';
		follow = true;
	}

	function onKey(e: KeyboardEvent) {
		if (!history.length) return;
		if (e.key === 'ArrowUp') {
			e.preventDefault();
			historyIdx = historyIdx < 0 ? history.length - 1 : Math.max(0, historyIdx - 1);
			input = history[historyIdx];
		} else if (e.key === 'ArrowDown') {
			e.preventDefault();
			if (historyIdx < 0) return;
			historyIdx++;
			if (historyIdx >= history.length) {
				historyIdx = -1;
				input = '';
			} else input = history[historyIdx];
		}
	}

	function lineClass(l: LogLine): string {
		switch (l.stream) {
			case 'sys':
				return 'text-sky-400';
			case 'in':
				return 'text-amber-300';
			case 'err':
				return 'text-red-400';
		}
		if (/\/(ERROR|FATAL)\]|\bERROR\]:/.test(l.text)) return 'text-red-400';
		if (/\/WARN\]|\bWARN\]:/.test(l.text)) return 'text-yellow-300';
		return 'text-zinc-300';
	}

	const time = (ts: number) => new Date(ts).toLocaleTimeString();
</script>

<div class="card flex flex-col overflow-hidden">
	<div class="flex items-center justify-between border-b border-zinc-800 px-4 py-2 text-xs text-zinc-400">
		<span class="font-medium tracking-wide uppercase">Console</span>
		<span class="flex items-center gap-3">
			{#if !follow}
				<button
					class="text-emerald-400 hover:underline"
					onclick={() => {
						follow = true;
						box?.scrollTo({ top: box.scrollHeight });
					}}>↓ follow</button
				>
			{/if}
			<span class="flex items-center gap-1.5">
				<span class="h-1.5 w-1.5 rounded-full {connected ? 'bg-emerald-400' : 'bg-red-400 animate-pulse'}"></span>
				{connected ? 'live' : 'reconnecting…'}
			</span>
		</span>
	</div>
	<div
		bind:this={box}
		onscroll={onScroll}
		class="h-[28rem] overflow-y-auto bg-black/40 px-4 py-3 font-mono text-[12.5px] leading-relaxed"
	>
		{#each lines as l (l.seq === 0 ? `e${l.ts}${l.text}` : l.seq)}
			<div class="break-all whitespace-pre-wrap {lineClass(l)}">
				{#if l.stream === 'sys'}<span class="text-zinc-600">[{time(l.ts)}] mcsd ›&nbsp;</span>{/if}{l.text}
			</div>
		{:else}
			<p class="text-zinc-600">No output yet.</p>
		{/each}
	</div>
	<form onsubmit={send} class="flex border-t border-zinc-800">
		<span class="flex items-center pl-4 font-mono text-emerald-500">&gt;</span>
		<input
			class="flex-1 bg-transparent px-3 py-3 font-mono text-sm outline-none placeholder:text-zinc-600 disabled:opacity-50"
			placeholder={active ? 'Type a command (e.g. say hello, op Steve) — ↑ for history' : 'Server is not running'}
			bind:value={input}
			onkeydown={onKey}
			disabled={!active}
			autocomplete="off"
			spellcheck="false"
		/>
	</form>
</div>
