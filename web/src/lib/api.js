// Thin client for the mcsd daemon API. In production the UI is served from the
// same origin as the daemon; in dev, Vite proxies /api to the daemon.

async function parse(res) {
  if (!res.ok) {
    let msg = res.statusText
    try {
      const body = await res.json()
      if (body && body.error) msg = body.error
    } catch {
      // non-JSON error body
    }
    throw new Error(msg)
  }
  return res.status === 204 ? null : res.json()
}

export const api = {
  list: () => fetch('/api/servers').then(parse),
  get: (id) => fetch(`/api/servers/${id}`).then(parse),
  create: (config) =>
    fetch('/api/servers', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(config),
    }).then(parse),
  remove: (id, purge = false) =>
    fetch(`/api/servers/${id}${purge ? '?purge=true' : ''}`, { method: 'DELETE' }).then(parse),
  start: (id) => fetch(`/api/servers/${id}/start`, { method: 'POST' }).then(parse),
  stop: (id) => fetch(`/api/servers/${id}/stop`, { method: 'POST' }).then(parse),
  restart: (id) => fetch(`/api/servers/${id}/restart`, { method: 'POST' }).then(parse),
  send: (id, command) =>
    fetch(`/api/servers/${id}/command`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ command }),
    }).then(parse),
  logs: (id, tail = 200) => fetch(`/api/servers/${id}/logs?tail=${tail}`).then(parse),
  streamUrl: (id, tail = 300) => `/api/servers/${id}/stream?tail=${tail}`,

  properties: (id) => fetch(`/api/servers/${id}/properties`).then(parse),
  saveProperties: (id, values) =>
    fetch(`/api/servers/${id}/properties`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ values }),
    }).then(parse),
  eula: (id) => fetch(`/api/servers/${id}/eula`).then(parse),
  acceptEula: (id, accepted) =>
    fetch(`/api/servers/${id}/eula`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ accepted }),
    }).then(parse),

  providers: () => fetch('/api/providers').then(parse),
  versions: (fork) => fetch(`/api/providers/${fork}/versions`).then(parse),
  builds: (fork, version) => fetch(`/api/providers/${fork}/versions/${version}/builds`).then(parse),
  java: (mc) => fetch(`/api/java${mc ? `?mc=${encodeURIComponent(mc)}` : ''}`).then(parse),
  install: (id, body) =>
    fetch(`/api/servers/${id}/install`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }).then(parse),
}
