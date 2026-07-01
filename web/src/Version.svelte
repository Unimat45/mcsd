<script>
  import { api } from './lib/api.js'

  let { server, onInstalled } = $props()

  let providers = $state([])
  let fork = $state('')
  let versions = $state([])
  let version = $state('')
  let builds = $state([])
  let build = $state('')

  let java = $state(null) // { installed, recommended }
  let preview = $state(null) // dryRun { artifact, warnings }
  let loading = $state(false)
  let installing = $state(false)
  let error = $state('')
  let status = $state('')

  let running = $derived(server?.state === 'running')
  let hasBuilds = $derived(providers.find((p) => p.id === fork)?.hasBuilds ?? false)

  async function init() {
    error = ''
    try {
      const [provs, j] = await Promise.all([api.providers(), api.java()])
      providers = provs
      java = j
      fork = server?.fork || provs[0]?.id || 'vanilla'
      await loadVersions()
    } catch (e) {
      error = e.message
    }
  }

  async function loadVersions() {
    error = ''
    preview = null
    loading = true
    try {
      const res = await api.versions(fork)
      versions = res.versions
      version = versions.includes(server?.mc_version) ? server.mc_version : versions[0] || ''
      await onVersionChange()
    } catch (e) {
      versions = []
      error = e.message
    } finally {
      loading = false
    }
  }

  async function onVersionChange() {
    preview = null
    build = ''
    if (!version) {
      builds = []
      return
    }
    if (hasBuilds) {
      try {
        const res = await api.builds(fork, version)
        builds = res.builds
        build = builds[0]?.id || ''
      } catch (e) {
        builds = []
        error = e.message
      }
    } else {
      builds = []
    }
    java = await api.java(version)
    await refreshPreview()
  }

  // Dry-run resolve to surface size + warnings before committing.
  async function refreshPreview() {
    if (!version) return
    try {
      preview = await api.install(server.id, { fork, mcVersion: version, build, dryRun: true })
    } catch (e) {
      preview = null
      error = e.message
    }
  }

  async function doInstall() {
    installing = true
    error = ''
    status = ''
    try {
      const res = await api.install(server.id, { fork, mcVersion: version, build })
      status = `Installed ${fork} ${version}${res.artifact.build ? ` build ${res.artifact.build}` : ''}.`
      onInstalled?.()
    } catch (e) {
      error = e.message
    } finally {
      installing = false
    }
  }

  function humanSize(bytes) {
    if (!bytes) return ''
    const mb = bytes / (1024 * 1024)
    return `${mb.toFixed(1)} MB`
  }

  // Only (re)initialize when the selected server id actually changes. The
  // parent re-polls every 2s, handing us a new `server` object each time; without
  // this guard the tab would refetch everything (and flicker) on every poll.
  let loadedFor = ''
  $effect(() => {
    const sid = server?.id
    if (sid && sid !== loadedFor) {
      loadedFor = sid
      init()
    }
  })
</script>

<div class="version">
  {#if error}<div class="banner err">{error}</div>{/if}

  <div class="current">
    Currently installed:
    {#if server?.fork}
      <b>{server.fork} {server.mc_version}{server.build ? ` (build ${server.build})` : ''}</b>
    {:else}
      <span class="muted">nothing yet</span>
    {/if}
  </div>

  <div class="selectors">
    <label>
      <span>Fork</span>
      <select bind:value={fork} onchange={loadVersions}>
        {#each providers as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
      </select>
    </label>

    <label>
      <span>Version</span>
      <select bind:value={version} onchange={onVersionChange} disabled={loading || versions.length === 0}>
        {#each versions as v (v)}<option value={v}>{v}</option>{/each}
      </select>
    </label>

    {#if hasBuilds}
      <label>
        <span>Build</span>
        <select bind:value={build} onchange={refreshPreview} disabled={builds.length === 0}>
          {#each builds as b (b.id)}
            <option value={b.id}>{b.id}{b.channel && b.channel !== 'STABLE' ? ` (${b.channel.toLowerCase()})` : ''}</option>
          {/each}
        </select>
      </label>
    {/if}
  </div>

  {#if java}
    <div class="java">
      <span>Java:</span>
      {#each java.installed as j}<span class="chip">{j.major} <span class="muted">({j.version})</span></span>{/each}
      {#if java.installed.length === 0}<span class="muted">none detected</span>{/if}
      {#if java.recommended}<span class="rec">recommends Java {java.recommended}</span>{/if}
    </div>
  {/if}

  {#if preview?.warnings?.length}
    <div class="banner warn">
      <ul>
        {#each preview.warnings as w}<li>{w}</li>{/each}
      </ul>
    </div>
  {/if}

  <div class="actions">
    {#if preview?.artifact}
      <span class="muted">{preview.artifact.fileName} · {humanSize(preview.artifact.size)}</span>
    {/if}
    {#if status}<span class="status">{status}</span>{/if}
    <button class="primary" onclick={doInstall} disabled={running || installing || !version}>
      {installing ? 'Installing…' : 'Install'}
    </button>
  </div>
  {#if running}
    <div class="note">Stop the server before changing its version.</div>
  {/if}
</div>

<style>
  .version { display: flex; flex-direction: column; gap: 14px; flex: 1; min-height: 0; overflow-y: auto; }
  .current { color: var(--text); }
  .muted { color: var(--muted); }

  .selectors { display: flex; gap: 16px; flex-wrap: wrap; }
  .selectors label { display: flex; flex-direction: column; gap: 4px; }
  .selectors span { font-size: 12px; color: var(--muted); }
  .selectors select { min-width: 180px; }

  .java { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: 13px; }
  .chip {
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px 10px;
  }
  .rec { color: var(--starting); }

  .banner { border-radius: 6px; padding: 10px 12px; }
  .banner.err { background: #3a1d1d; border: 1px solid var(--crashed); color: #f2b8b8; }
  .banner.warn { background: #3a331d; border: 1px solid var(--starting); color: #f0dca0; }
  .banner ul { margin: 0; padding-left: 18px; }

  .actions { display: flex; align-items: center; gap: 12px; }
  .status { color: var(--running); font-size: 13px; }
  button.primary { border-color: var(--accent); }
  .note { color: var(--muted); font-size: 12px; }
</style>
