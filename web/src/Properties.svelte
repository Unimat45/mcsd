<script>
  import { api } from './lib/api.js'

  let { id } = $props()

  let data = $state(null) // { path, exists, values, order, schema }
  let eula = $state(null) // { exists, accepted }
  let working = $state({}) // key -> string (edited copy)
  let initial = {} // baseline for change detection (plain, not reactive)
  let error = $state('')
  let status = $state('')
  let saving = $state(false)

  async function load() {
    error = ''
    status = ''
    try {
      const [props, e] = await Promise.all([api.properties(id), api.eula(id)])
      const model = {}
      for (const s of props.schema) model[s.key] = props.values[s.key] ?? s.default
      for (const k of props.order) if (!(k in model)) model[k] = props.values[k]
      data = props
      eula = e
      working = model
      initial = { ...model }
    } catch (err) {
      error = err.message
    }
  }

  $effect(() => {
    void id
    load()
  })

  let schemaKeys = $derived.by(() => new Set((data?.schema ?? []).map((s) => s.key)))
  let categories = $derived.by(() => {
    const cats = []
    for (const s of data?.schema ?? []) if (!cats.includes(s.category)) cats.push(s.category)
    return cats
  })
  let extras = $derived.by(() => (data?.order ?? []).filter((k) => !schemaKeys.has(k)))
  let dirty = $derived.by(() => {
    for (const k in working) if (working[k] !== initial[k]) return true
    return false
  })

  function specsFor(cat) {
    return (data?.schema ?? []).filter((s) => s.category === cat)
  }

  function buildPayload() {
    const fileKeys = new Set(data?.order ?? [])
    const out = {}
    for (const k in working) {
      // Persist keys already in the file (to preserve them) and any the user
      // changed from their default/baseline.
      if (fileKeys.has(k) || working[k] !== initial[k]) out[k] = working[k]
    }
    return out
  }

  async function save() {
    saving = true
    error = ''
    status = ''
    try {
      data = await api.saveProperties(id, buildPayload())
      initial = { ...working }
      status = 'Saved. Changes take effect on next start.'
    } catch (err) {
      error = err.message
    } finally {
      saving = false
    }
  }

  async function acceptEula() {
    try {
      eula = await api.acceptEula(id, true)
    } catch (err) {
      error = err.message
    }
  }
</script>

<div class="props">
  {#if error}<div class="banner err">{error}</div>{/if}

  {#if eula && !eula.accepted}
    <div class="banner warn">
      <div>
        The Minecraft <b>EULA</b> must be accepted before the server will start.
      </div>
      <button onclick={acceptEula}>Accept EULA</button>
    </div>
  {/if}

  {#if data}
    <div class="toolbar">
      <div class="path">
        {data.path}
        {#if !data.exists}<span class="new">(will be created)</span>{/if}
      </div>
      <div class="actions">
        {#if status}<span class="status">{status}</span>{/if}
        <button onclick={load} disabled={saving}>Reload</button>
        <button class="primary" onclick={save} disabled={!dirty || saving}>
          {saving ? 'Saving…' : 'Save'}
        </button>
      </div>
    </div>

    <div class="fields">
      {#each categories as cat}
        <section>
          <h3>{cat}</h3>
          {#each specsFor(cat) as spec (spec.key)}
            <label class="field">
              <div class="meta">
                <span class="key">{spec.key}</span>
                <span class="desc">{spec.description}</span>
              </div>
              <div class="control">
                {#if spec.type === 'bool'}
                  <input
                    type="checkbox"
                    checked={working[spec.key] === 'true'}
                    onchange={(e) => (working[spec.key] = e.currentTarget.checked ? 'true' : 'false')}
                  />
                {:else if spec.type === 'enum'}
                  <select bind:value={working[spec.key]}>
                    {#each spec.options as opt}<option value={opt}>{opt}</option>{/each}
                  </select>
                {:else if spec.type === 'int'}
                  <input
                    type="number"
                    value={working[spec.key]}
                    oninput={(e) => (working[spec.key] = e.currentTarget.value)}
                  />
                {:else}
                  <input type="text" bind:value={working[spec.key]} />
                {/if}
              </div>
            </label>
          {/each}
        </section>
      {/each}

      {#if extras.length > 0}
        <section>
          <h3>Other</h3>
          {#each extras as key (key)}
            <label class="field">
              <div class="meta"><span class="key">{key}</span></div>
              <div class="control">
                <input type="text" bind:value={working[key]} />
              </div>
            </label>
          {/each}
        </section>
      {/if}
    </div>
  {/if}
</div>

<style>
  .props {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    border-radius: 6px;
    padding: 10px 12px;
  }
  .banner.err { background: #3a1d1d; border: 1px solid var(--crashed); color: #f2b8b8; }
  .banner.warn { background: #3a331d; border: 1px solid var(--starting); color: #f0dca0; }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    position: sticky;
    top: 0;
    background: var(--bg);
    padding-bottom: 4px;
  }
  .path { color: var(--muted); font-family: ui-monospace, Menlo, monospace; font-size: 12px; }
  .new { color: var(--starting); }
  .actions { display: flex; align-items: center; gap: 10px; }
  .status { color: var(--running); font-size: 12px; }
  button.primary { border-color: var(--accent); }

  section { margin-bottom: 8px; }
  section h3 {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--muted);
    border-bottom: 1px solid var(--border);
    padding-bottom: 6px;
    margin: 8px 0;
  }
  .field {
    display: grid;
    grid-template-columns: 1fr 260px;
    gap: 16px;
    align-items: center;
    padding: 7px 4px;
  }
  .meta { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .key { font-family: ui-monospace, Menlo, monospace; font-size: 13px; }
  .desc { color: var(--muted); font-size: 12px; }
  .control { display: flex; justify-content: flex-end; }
  .control input[type='text'],
  .control input[type='number'],
  .control select {
    width: 100%;
  }
  .control input[type='checkbox'] { width: 18px; height: 18px; }
</style>
