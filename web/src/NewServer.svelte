<script>
  import { api } from './lib/api.js'

  let { onCreated, onClose } = $props()

  let name = $state('')
  let javaPath = $state('java')
  let memoryGB = $state('')
  let extraJvm = $state('')
  let serverArgs = $state('nogui')
  let autostart = $state(false)
  let autorestart = $state(true)
  let eulaAccepted = $state(false)

  let error = $state('')
  let saving = $state(false)

  function words(s) {
    return s.trim() ? s.trim().split(/\s+/) : []
  }

  async function submit(e) {
    e.preventDefault()
    if (!name.trim()) {
      error = 'Name is required.'
      return
    }
    if (!eulaAccepted) {
      error = 'You must accept the Minecraft EULA to create a server.'
      return
    }
    const javaArgs = []
    if (memoryGB) {
      javaArgs.push(`-Xmx${memoryGB}G`, `-Xms${memoryGB}G`)
    }
    javaArgs.push(...words(extraJvm))

    const config = {
      name: name.trim(),
      java_path: javaPath.trim() || 'java',
      java_args: javaArgs,
      server_args: words(serverArgs),
      autostart,
      autorestart,
    }

    saving = true
    error = ''
    try {
      const created = await api.create(config)
      if (eulaAccepted) {
        await api.acceptEula(created.id, true)
      }
      onCreated?.(created.id)
    } catch (err) {
      error = err.message
    } finally {
      saving = false
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && onClose()} />

<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onClose()} role="presentation">
  <form class="modal" onsubmit={submit}>
    <h2>New server</h2>
    {#if error}<div class="err">{error}</div>{/if}

    <label class="field">
      <span>Name <em>(required)</em></span>
      <input bind:value={name} placeholder="Survival" autocomplete="off" />
    </label>

    <div class="row2">
      <label class="field">
        <span>Java path</span>
        <input bind:value={javaPath} placeholder="java" />
      </label>
      <label class="field">
        <span>Memory (GB)</span>
        <input type="number" min="1" bind:value={memoryGB} placeholder="4" />
      </label>
    </div>

    <label class="field">
      <span>Extra JVM args</span>
      <input bind:value={extraJvm} placeholder="-XX:+UseG1GC" />
    </label>
    <label class="field">
      <span>Server args</span>
      <input bind:value={serverArgs} placeholder="nogui" />
    </label>

    <div class="checks">
      <label><input type="checkbox" bind:checked={autostart} /> Auto-start on daemon boot</label>
      <label><input type="checkbox" bind:checked={autorestart} /> Auto-restart on crash</label>
      <label class="eula">
        <input type="checkbox" bind:checked={eulaAccepted} />
        I accept the
        <a href="https://aka.ms/MinecraftEULA" target="_blank" rel="noreferrer">Minecraft EULA</a>
        <em>(required)</em>
      </label>
    </div>

    <div class="note">
      Files are stored automatically in the daemon's servers directory. Accepting
      the EULA writes <code>eula.txt</code> for you; after creating, use the
      <b>Version</b> tab to install a jar.
    </div>

    <div class="actions">
      <button type="button" onclick={onClose} disabled={saving}>Cancel</button>
      <button type="submit" class="primary" disabled={saving || !name.trim() || !eulaAccepted}>
        {saving ? 'Creating…' : 'Create'}
      </button>
    </div>
  </form>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10;
  }
  .modal {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 20px 22px;
    width: 460px;
    max-width: calc(100vw - 32px);
    max-height: calc(100vh - 32px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .modal h2 { margin: 0 0 4px; font-size: 18px; }
  .err {
    background: #3a1d1d;
    border: 1px solid var(--crashed);
    color: #f2b8b8;
    border-radius: 6px;
    padding: 8px 12px;
  }
  .field { display: flex; flex-direction: column; gap: 4px; }
  .field span { font-size: 12px; color: var(--muted); }
  .field em { font-style: normal; opacity: 0.7; }
  .field input { width: 100%; }
  .row2 { display: grid; grid-template-columns: 2fr 1fr; gap: 12px; }
  .checks { display: flex; flex-direction: column; gap: 6px; }
  .checks label { display: flex; align-items: center; gap: 8px; font-size: 13px; color: var(--text); }
  .checks input { width: 16px; height: 16px; }
  .eula a { color: var(--accent); }
  .eula em { font-style: normal; color: var(--muted); }
  .note code { font-family: ui-monospace, Menlo, monospace; }
  .note { color: var(--muted); font-size: 12px; }
  .actions { display: flex; justify-content: flex-end; gap: 10px; margin-top: 4px; }
  button.primary { border-color: var(--accent); }
</style>
