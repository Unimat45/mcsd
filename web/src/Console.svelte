<script>
  import { untrack } from 'svelte'
  import { api } from './lib/api.js'

  let { id, running } = $props()

  // The parent recreates <Console> (via {#key}) when the selected server
  // changes, so capture the id once. untrack() makes the "initial value only"
  // intent explicit: it keeps the SSE effect below from re-running on the
  // parent's 2s status poll, which would otherwise close and reopen the stream
  // and replay the entire log (the "redraw on every poll").
  const streamId = untrack(() => id)

  let lines = $state([])
  let command = $state('')
  let sendError = $state('')
  let box
  let pinned = true // auto-follow the tail only while scrolled to the bottom

  const CAP = 3000 // bound the DOM/memory for long-lived sessions

  // Open the SSE stream once for this server. The endpoint replays recent
  // history then streams live lines (and survives server restarts). This effect
  // reads no reactive state, so it runs a single time on mount and tears down on
  // destroy — it is NOT re-run by the parent's status poll.
  //
  // Incoming lines are buffered and flushed once per animation frame: a busy
  // server can emit hundreds of lines per second, and reacting to each one
  // individually floods the DOM with re-renders (visible as flicker). Each line
  // gets a stable key so trimming the head never re-renders the survivors.
  $effect(() => {
    let seq = 0
    let buffer = []
    let raf = 0
    const flush = () => {
      raf = 0
      if (buffer.length === 0) return
      lines.push(...buffer)
      buffer = []
      if (lines.length > CAP) lines.splice(0, lines.length - CAP)
    }

    const es = new EventSource(api.streamUrl(streamId))
    es.onmessage = (e) => {
      try {
        const line = JSON.parse(e.data)
        if (line.stream === 'reset') {
          // Server cleared its logs on a new run — reset the live view too.
          buffer = []
          lines = []
          return
        }
        line._k = seq++
        buffer.push(line)
        if (!raf) raf = requestAnimationFrame(flush)
      } catch {
        // ignore malformed frame
      }
    }
    return () => {
      es.close()
      if (raf) cancelAnimationFrame(raf)
    }
  })

  // Follow the tail as the log grows, but only if the user hasn't scrolled up.
  $effect(() => {
    lines.length
    if (pinned && box) box.scrollTop = box.scrollHeight
  })

  function onScroll() {
    if (!box) return
    pinned = box.scrollHeight - box.scrollTop - box.clientHeight < 40
  }

  async function submit(e) {
    e.preventDefault()
    const c = command.trim()
    if (!c) return
    try {
      await api.send(id, c)
      command = ''
      sendError = ''
    } catch (err) {
      sendError = err.message
    }
  }

  function fmtTime(ts) {
    const d = new Date(ts)
    return d.toLocaleTimeString([], { hour12: false })
  }
</script>

<div class="console">
  <div class="lines" bind:this={box} onscroll={onScroll}>
    {#each lines as line (line._k)}
      <div class="line {line.stream}">
        <span class="ts">{fmtTime(line.ts)}</span>
        <span class="text">{line.text}</span>
      </div>
    {/each}
  </div>

  <form class="prompt" onsubmit={submit}>
    <input
      placeholder={running ? 'type a console command…' : 'server not running'}
      bind:value={command}
      disabled={!running}
    />
    <button type="submit" disabled={!running}>Send</button>
  </form>
  {#if sendError}<div class="send-error">{sendError}</div>{/if}
</div>

<style>
  .console {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    background: #0b0d11;
  }

  .lines {
    flex: 1;
    overflow-y: auto;
    overflow-anchor: none; /* we manage scroll position ourselves */
    padding: 10px 12px;
    font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-size: 12.5px;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .line { display: flex; gap: 10px; }
  .ts { color: #4b5563; flex: none; user-select: none; }
  .text { flex: 1; }
  .line.stderr .text { color: #f28b82; }
  .line.system .text { color: #7aa2f7; font-style: italic; }

  .prompt {
    display: flex;
    gap: 8px;
    padding: 10px;
    border-top: 1px solid var(--border);
    background: var(--panel);
  }
  .prompt input { flex: 1; font-family: ui-monospace, Menlo, monospace; }

  .send-error {
    color: #f28b82;
    padding: 6px 12px;
    font-size: 12px;
    background: var(--panel);
  }
</style>
