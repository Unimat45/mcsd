<script>
  import { api } from './lib/api.js'

  let { server, onDeleted, onClose } = $props()

  let purge = $state(false)
  let deleting = $state(false)
  let error = $state('')

  let running = $derived(['running', 'starting', 'stopping'].includes(server?.state))

  async function confirm() {
    deleting = true
    error = ''
    try {
      await api.remove(server.id, purge)
      onDeleted?.()
    } catch (err) {
      error = err.message
    } finally {
      deleting = false
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && onClose()} />

<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onClose()} role="presentation">
  <form class="modal" onsubmit={(e) => { e.preventDefault(); confirm() }}>
    <h2>Delete server</h2>
    {#if error}<div class="err">{error}</div>{/if}

    <p>
      Delete <b>{server?.name || server?.id}</b>? This removes it from the daemon.
    </p>

    {#if running}
      <div class="warn">Stop the server before deleting it.</div>
    {/if}

    <label class="purge">
      <input type="checkbox" bind:checked={purge} disabled={running} />
      Also delete all files from disk (world data, configs, jar) — <b>cannot be undone</b>
    </label>

    <div class="actions">
      <button type="button" onclick={onClose} disabled={deleting}>Cancel</button>
      <button type="submit" class="danger" disabled={deleting || running}>
        {deleting ? 'Deleting…' : purge ? 'Delete + files' : 'Delete'}
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
    width: 440px;
    max-width: calc(100vw - 32px);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .modal h2 { margin: 0; font-size: 18px; }
  .modal p { margin: 0; }
  .err, .warn {
    border-radius: 6px;
    padding: 8px 12px;
    font-size: 13px;
  }
  .err { background: #3a1d1d; border: 1px solid var(--crashed); color: #f2b8b8; }
  .warn { background: #3a331d; border: 1px solid var(--starting); color: #f0dca0; }
  .purge {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    font-size: 13px;
    color: var(--text);
  }
  .purge input { width: 16px; height: 16px; margin-top: 2px; flex: none; }
  .actions { display: flex; justify-content: flex-end; gap: 10px; margin-top: 4px; }
  button.danger { border-color: var(--crashed); color: #f2b8b8; }
  button.danger:hover:not(:disabled) { border-color: var(--crashed); background: #3a1d1d; }
</style>
