<script lang="ts">
	import { goto } from '$app/navigation';
	import { api, auth } from '$lib/api';

	let token = $state('');
	let error = $state('');
	let busy = $state(false);

	async function login(e: SubmitEvent) {
		e.preventDefault();
		busy = true;
		error = '';
		try {
			await api('/servers', { token: token.trim() });
			auth.set(token);
			goto('/');
		} catch (err) {
			error = (err as Error).message;
		} finally {
			busy = false;
		}
	}
</script>

<div class="mx-auto mt-16 max-w-md">
	<form class="card space-y-5 p-8" onsubmit={login}>
		<div>
			<h1 class="text-xl font-semibold">Connect to mcsd</h1>
			<p class="mt-2 text-sm text-zinc-400">
				Run <code class="rounded bg-zinc-800 px-1.5 py-0.5 text-emerald-300">mcsd dashboard</code> on the machine
				running the daemon and open the printed link, or paste the token from its <code>config.toml</code>.
			</p>
		</div>
		<div>
			<label class="label" for="token">API token</label>
			<input id="token" class="input font-mono" type="password" bind:value={token} autocomplete="current-password" />
		</div>
		{#if error}<p class="text-sm text-red-400">{error}</p>{/if}
		<button class="btn btn-primary w-full !py-2" disabled={busy || !token.trim()}>{busy ? 'Checking…' : 'Log in'}</button>
	</form>
</div>
