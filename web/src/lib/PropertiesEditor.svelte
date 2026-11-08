<script lang="ts">
	import { api, type PropertiesView } from './api';
	import { KNOWN_KEYS, PROPERTY_GROUPS, validate, type PropDef } from './properties';

	let { name, running }: { name: string; running: boolean } = $props();
	const serverName = $derived(name);

	const defs = new Map<string, PropDef>(PROPERTY_GROUPS.flatMap((g) => g.props.map((p) => [p.key, p] as const)));

	let view = $state<PropertiesView | null>(null);
	let original = $state<Record<string, string>>({});
	let edits = $state<Record<string, string>>({});
	let removed = $state<string[]>([]);
	let mode = $state<'form' | 'raw'>('form');
	let rawText = $state('');
	let filter = $state('');
	let newKey = $state('');
	let newValue = $state('');
	let saving = $state(false);
	let loadError = $state('');
	let message = $state<{ kind: 'ok' | 'error'; text: string } | null>(null);
	let pendingRestart = $state(false);

	function apply(v: PropertiesView) {
		view = v;
		original = Object.fromEntries(v.entries.map((e) => [e.key, e.value]));
		edits = {};
		removed = [];
		rawText = v.raw;
	}

	async function load() {
		loadError = '';
		try {
			apply(await api<PropertiesView>(`/servers/${serverName}/properties`));
		} catch (e) {
			loadError = (e as Error).message;
		}
	}

	$effect(() => {
		serverName;
		view = null;
		message = null;
		pendingRestart = false;
		load();
	});

	const base = (key: string) => (key in original ? original[key] : (defs.get(key)?.default ?? ''));
	const valueOf = (key: string) => edits[key] ?? base(key);
	const isChanged = (key: string) => key in edits || removed.includes(key);

	function setValue(key: string, value: string) {
		if (value === base(key)) delete edits[key];
		else edits[key] = value;
	}

	function removeKey(key: string) {
		delete edits[key];
		if (key in original && !removed.includes(key)) removed.push(key);
	}

	function addKey(e: SubmitEvent) {
		e.preventDefault();
		const key = newKey.trim();
		if (!key) return;
		removed = removed.filter((k) => k !== key);
		edits[key] = newValue;
		newKey = '';
		newValue = '';
	}

	const matches = (key: string, label = '') => {
		const q = filter.trim().toLowerCase();
		return !q || key.toLowerCase().includes(q) || label.toLowerCase().includes(q);
	};

	// After the first start Minecraft writes every key it supports, so hide
	// catalog keys this version doesn't have (e.g. ones moved to game rules).
	const groups = $derived(
		PROPERTY_GROUPS.map((g) => ({
			title: g.title,
			props: g.props.filter((p) => (!view?.generated || p.key in original) && matches(p.key, p.label))
		})).filter((g) => g.props.length)
	);
	const otherKeys = $derived(
		[...Object.keys(original), ...Object.keys(edits).filter((k) => !(k in original))].filter(
			(k) => !KNOWN_KEYS.has(k) && matches(k)
		)
	);
	const errors = $derived(
		Object.fromEntries(
			Object.entries(edits)
				.map(([k, v]) => [k, validate(defs.get(k), v)] as const)
				.filter(([, err]) => err)
		) as Record<string, string>
	);
	const changeCount = $derived(Object.keys(edits).length + removed.length);
	const rawDirty = $derived(!!view && rawText !== view.raw);
	const newKeyValid = $derived(/^[^\s=:#!\\]{1,128}$/.test(newKey.trim()));

	async function save() {
		saving = true;
		message = null;
		try {
			const v =
				mode === 'raw'
					? await api<PropertiesView>(`/servers/${serverName}/properties`, { method: 'PUT', body: { raw: rawText } })
					: await api<PropertiesView>(`/servers/${serverName}/properties`, {
							method: 'PATCH',
							body: { set: { ...edits }, remove: [...removed] }
						});
			apply(v);
			pendingRestart = v.restart_required;
			message = { kind: 'ok', text: v.restart_required ? 'Saved. Restart the server to apply.' : 'Saved.' };
		} catch (e) {
			message = { kind: 'error', text: (e as Error).message };
		} finally {
			saving = false;
		}
	}

	function discard() {
		if (view) apply(view);
		message = null;
	}

	function switchMode(m: 'form' | 'raw') {
		if (m === mode) return;
		const dirty = mode === 'form' ? changeCount > 0 : rawDirty;
		if (dirty && !confirm('Discard unsaved changes?')) return;
		discard();
		mode = m;
	}

	async function restart() {
		try {
			await api(`/servers/${serverName}/restart?wait=false`, { method: 'POST' });
			pendingRestart = false;
			message = { kind: 'ok', text: 'Restarting…' };
		} catch (e) {
			message = { kind: 'error', text: (e as Error).message };
		}
	}
</script>

{#snippet field(def: PropDef)}
	{@const value = valueOf(def.key)}
	{@const changed = isChanged(def.key)}
	<div class="flex flex-col gap-1">
		<label class="flex items-center gap-1.5 text-xs font-medium text-zinc-400" for="prop-{def.key}">
			{#if changed}<span class="h-1.5 w-1.5 rounded-full bg-amber-400" title="Modified"></span>{/if}
			{def.label}
			<code class="ml-auto font-normal text-zinc-600">{def.key}</code>
		</label>
		{#if def.type === 'bool'}
			<label class="flex h-9 cursor-pointer items-center gap-2 text-sm">
				<input
					id="prop-{def.key}"
					type="checkbox"
					class="peer sr-only"
					checked={value === 'true'}
					onchange={(e) => setValue(def.key, String(e.currentTarget.checked))}
				/>
				<span
					class="relative h-5 w-9 rounded-full bg-zinc-700 transition-colors peer-checked:bg-emerald-600 after:absolute after:top-0.5 after:left-0.5 after:h-4 after:w-4 after:rounded-full after:bg-white after:transition-transform peer-checked:after:translate-x-4"
				></span>
				<span class="text-zinc-400">{value === 'true' ? 'On' : 'Off'}</span>
			</label>
		{:else if def.type === 'enum'}
			<select id="prop-{def.key}" class="input" {value} onchange={(e) => setValue(def.key, e.currentTarget.value)}>
				{#if !def.options?.includes(value)}<option {value}>{value || '(empty)'}</option>{/if}
				{#each def.options ?? [] as opt}<option value={opt}>{opt}</option>{/each}
			</select>
		{:else}
			<input
				id="prop-{def.key}"
				class="input {def.type === 'int' ? '' : 'font-mono'} {errors[def.key] ? '!border-red-500' : ''}"
				type={def.type === 'secret' ? 'password' : def.type === 'int' ? 'number' : 'text'}
				min={def.min}
				max={def.max}
				{value}
				placeholder={def.default ? `default: ${def.default}` : ''}
				autocomplete={def.type === 'secret' ? 'new-password' : 'off'}
				oninput={(e) => setValue(def.key, e.currentTarget.value)}
			/>
		{/if}
		{#if errors[def.key]}
			<p class="text-xs text-red-400">{errors[def.key]}</p>
		{:else if def.help}
			<p class="text-xs text-zinc-500">{def.help}</p>
		{/if}
	</div>
{/snippet}

<div class="card">
	<div class="flex flex-wrap items-center gap-3 border-b border-zinc-800 px-5 py-3">
		<h2 class="font-semibold">server.properties</h2>
		<div class="flex rounded-lg border border-zinc-700 p-0.5 text-xs">
			{#each [['form', 'Form'], ['raw', 'Raw file']] as [m, label]}
				<button
					class="rounded-md px-2.5 py-1 {mode === m ? 'bg-zinc-700 text-white' : 'text-zinc-400 hover:text-zinc-200'}"
					onclick={() => switchMode(m as 'form' | 'raw')}>{label}</button
				>
			{/each}
		</div>
		{#if mode === 'form'}
			<input class="input ml-auto !w-56 !py-1.5" placeholder="Filter properties…" bind:value={filter} />
		{/if}
		<button class="btn btn-ghost !py-1 text-xs {mode === 'form' ? '' : 'ml-auto'}" onclick={load}>Reload</button>
	</div>

	{#if loadError}
		<p class="p-5 text-sm text-red-400">{loadError}</p>
	{:else if !view}
		<p class="p-5 text-sm text-zinc-500">Loading…</p>
	{:else}
		{#if !view.generated}
			<p class="mx-5 mt-4 rounded-lg border border-sky-900 bg-sky-950/40 p-3 text-sm text-sky-200">
				Minecraft writes the full file the first time the server starts. Values you set now are kept and used on that first start.
			</p>
		{/if}

		{#if mode === 'form'}
			<div class="space-y-8 p-5">
				{#each groups as group (group.title)}
					<section>
						<h3 class="mb-3 text-xs font-semibold tracking-wide text-zinc-500 uppercase">{group.title}</h3>
						<div class="grid gap-x-6 gap-y-4 sm:grid-cols-2 lg:grid-cols-3">
							{#each group.props as def (def.key)}{@render field(def)}{/each}
						</div>
					</section>
				{/each}

				<section>
					<h3 class="mb-3 text-xs font-semibold tracking-wide text-zinc-500 uppercase">Other properties</h3>
					{#if otherKeys.length}
						<div class="divide-y divide-zinc-800 rounded-lg border border-zinc-800">
							{#each otherKeys as key (key)}
								{@const gone = removed.includes(key)}
								<div class="flex items-center gap-3 px-3 py-2">
									<code class="w-72 shrink-0 truncate text-xs {gone ? 'text-zinc-600 line-through' : 'text-zinc-300'}" title={key}>
										{#if isChanged(key) && !gone}<span class="mr-1 inline-block h-1.5 w-1.5 rounded-full bg-amber-400"></span>{/if}{key}
									</code>
									<input
										class="input !py-1.5 font-mono"
										value={valueOf(key)}
										disabled={gone}
										oninput={(e) => setValue(key, e.currentTarget.value)}
									/>
									{#if gone}
										<button class="text-xs text-zinc-400 hover:text-zinc-200" onclick={() => (removed = removed.filter((k) => k !== key))}>Undo</button>
									{:else}
										<button class="px-1 text-zinc-500 hover:text-red-400" title="Remove" onclick={() => removeKey(key)}>✕</button>
									{/if}
								</div>
							{/each}
						</div>
					{:else if !filter}
						<p class="text-sm text-zinc-500">No other properties.</p>
					{/if}
					{#if !filter}
						<form class="mt-3 flex gap-2" onsubmit={addKey}>
							<input class="input !w-72 !py-1.5 font-mono" placeholder="new-property" bind:value={newKey} />
							<input class="input !py-1.5 font-mono" placeholder="value" bind:value={newValue} />
							<button class="btn btn-ghost shrink-0" disabled={!newKeyValid}>Add</button>
						</form>
					{/if}
				</section>
			</div>
		{:else}
			<div class="p-5">
				<textarea
					class="input h-[32rem] font-mono text-xs leading-relaxed"
					bind:value={rawText}
					spellcheck="false"
					placeholder="key=value, one per line"
				></textarea>
				<p class="mt-2 text-xs text-zinc-500">
					Saved as-is, except non-ASCII characters, which are stored as \uXXXX escapes so every Minecraft version reads them correctly.
				</p>
			</div>
		{/if}

		<div class="sticky bottom-0 flex flex-wrap items-center justify-end gap-3 rounded-b-xl border-t border-zinc-800 bg-zinc-900/95 px-5 py-3 backdrop-blur">
			{#if message}
				<span class="mr-auto text-sm {message.kind === 'error' ? 'text-red-400' : 'text-zinc-300'}">{message.text}</span>
			{/if}
			{#if pendingRestart && running}
				<button class="btn btn-ghost" onclick={restart}>Restart now</button>
			{/if}
			{#if mode === 'form'}
				<button class="btn btn-ghost" disabled={!changeCount || saving} onclick={discard}>Discard</button>
				<button class="btn btn-primary" disabled={!changeCount || saving || Object.keys(errors).length > 0} onclick={save}>
					{saving ? 'Saving…' : changeCount ? `Save ${changeCount} change${changeCount === 1 ? '' : 's'}` : 'No changes'}
				</button>
			{:else}
				<button class="btn btn-ghost" disabled={!rawDirty || saving} onclick={discard}>Discard</button>
				<button class="btn btn-primary" disabled={!rawDirty || saving} onclick={save}>{saving ? 'Saving…' : 'Save file'}</button>
			{/if}
		</div>
	{/if}
</div>
