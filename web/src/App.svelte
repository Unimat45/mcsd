<script>
  import { onMount } from 'svelte'
  import { api } from './lib/api.js'
  import Console from './Console.svelte'
  import Properties from './Properties.svelte'
  import Version from './Version.svelte'
  import NewServer from './NewServer.svelte'
  import DeleteServer from './DeleteServer.svelte'

  let servers = $state([])
  let selectedId = $state(null)
  let tab = $state('console')
  let error = $state('')
  let showAdd = $state(false)
  let showDelete = $state(false)

  let selected = $derived(servers.find((s) => s.id === selectedId) ?? null)
  let isRunning = $derived(selected?.state === 'running')
  let isBusy = $derived(['starting', 'stopping'].includes(selected?.state))

  async function refresh() {
    try {
      servers = await api.list()
      if (selectedId === null && servers.length > 0) selectedId = servers[0].id
      error = ''
    } catch (e) {
      error = e.message
    }
  }

  async function act(fn) {
    try {
      await fn()
      await refresh()
    } catch (e) {
      error = e.message
    }
  }

  function onCreated(newId) {
    showAdd = false
    selectedId = newId
    refresh()
  }

  function onDeleted() {
    showDelete = false
    selectedId = null
    refresh()
  }

  onMount(() => {
    refresh()
    const timer = setInterval(refresh, 2000)
    return () => clearInterval(timer)
  })
</script>

<div class="layout">
  <aside>
    <div class="brand">
      <h1>Miecraft Server Daemon</h1>
      <button class="add" onclick={() => (showAdd = true)} title="Add server">+</button>
    </div>
    {#if servers.length === 0}
      <p class="empty">No servers yet. Click + to add one.</p>
    {/if}
    <ul>
      {#each servers as s (s.id)}
        <li class:active={s.id === selectedId}>
          <button class="row" onclick={() => (selectedId = s.id)}>
            <span class="dot" style="background: var(--{s.state})"></span>
            <span class="name">{s.name || s.id}</span>
            <span class="state">{s.state}</span>
          </button>
        </li>
      {/each}
    </ul>
  </aside>

  <main>
    {#if error}
      <div class="banner">{error}</div>
    {/if}

    {#if selected}
      <header>
        <div class="title">
          <h2>{selected.name || selected.id}</h2>
          <span class="badge" style="border-color: var(--{selected.state}); color: var(--{selected.state})">
            {selected.state}
          </span>
        </div>
        <div class="controls">
          <button onclick={() => act(() => api.start(selected.id))} disabled={isRunning || isBusy}>Start</button>
          <button onclick={() => act(() => api.stop(selected.id))} disabled={!isRunning && !isBusy}>Stop</button>
          <button onclick={() => act(() => api.restart(selected.id))}>Restart</button>
          <button class="danger" onclick={() => (showDelete = true)} disabled={isRunning || isBusy} title="Delete server">Delete</button>
        </div>
      </header>

      <nav class="tabs">
        <button class:active={tab === 'console'} onclick={() => (tab = 'console')}>Console</button>
        <button class:active={tab === 'properties'} onclick={() => (tab = 'properties')}>Properties</button>
        <button class:active={tab === 'version'} onclick={() => (tab = 'version')}>Version</button>
      </nav>

      {#key selected.id}
        {#if tab === 'console'}
          <Console id={selected.id} running={isRunning} />
        {:else if tab === 'properties'}
          <Properties id={selected.id} />
        {:else}
          <Version server={selected} onInstalled={refresh} />
        {/if}
      {/key}
    {:else}
      <div class="placeholder">Select a server, or click + to add one.</div>
    {/if}
  </main>
</div>

{#if showAdd}
  <NewServer {onCreated} onClose={() => (showAdd = false)} />
{/if}

{#if showDelete && selected}
  <DeleteServer server={selected} {onDeleted} onClose={() => (showDelete = false)} />
{/if}

<style>
  .layout {
    display: grid;
    grid-template-columns: 260px 1fr;
    grid-template-rows: 100%; /* pin the row to the viewport so panes scroll internally */
    height: 100%;
    overflow: hidden;
  }

  aside {
    background: var(--panel);
    border-right: 1px solid var(--border);
    padding: 16px;
    overflow-y: auto;
  }
  .brand {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 4px 8px 16px;
  }
  .add {
    width: 28px;
    height: 28px;
    padding: 0;
    font-size: 18px;
    line-height: 1;
    border-radius: 6px;
  }
  aside h1 {
    font-size: 18px;
    margin: 0;
    letter-spacing: 0.5px;
  }
  aside ul { list-style: none; margin: 0; padding: 0; }
  aside li { margin-bottom: 4px; }
  .empty { color: var(--muted); padding: 0 8px; }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    padding: 8px 10px;
  }
  li.active .row { background: var(--panel-2); border-color: var(--border); }
  .dot { width: 9px; height: 9px; border-radius: 50%; flex: none; }
  .name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .state { color: var(--muted); font-size: 12px; }

  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0; /* allow the flex child (console/properties) to shrink and scroll */
    overflow: hidden;
    padding: 16px 20px;
    gap: 12px;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .title { display: flex; align-items: center; gap: 12px; }
  .title h2 { margin: 0; font-size: 18px; }
  .badge {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    border: 1px solid;
    border-radius: 999px;
    padding: 2px 10px;
  }
  .controls { display: flex; gap: 8px; }
  .controls .danger { color: #f2b8b8; }
  .controls .danger:hover:not(:disabled) { border-color: var(--crashed); background: #3a1d1d; }

  .tabs { display: flex; gap: 4px; border-bottom: 1px solid var(--border); }
  .tabs button {
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    color: var(--muted);
    padding: 6px 12px;
  }
  .tabs button.active { color: var(--text); border-bottom-color: var(--accent); }

  .banner {
    background: #3a1d1d;
    border: 1px solid var(--crashed);
    color: #f2b8b8;
    border-radius: 6px;
    padding: 8px 12px;
  }
  .placeholder { color: var(--muted); margin: auto; }
</style>
