import { goto } from '$app/navigation';

export type Status = 'stopped' | 'installing' | 'starting' | 'running' | 'stopping' | 'crashed' | 'error';
export type Kind = 'vanilla' | 'paper' | 'fabric' | 'custom';

export interface ServerInfo {
	name: string;
	kind: Kind;
	version: string;
	memory_mb: number;
	port: number;
	java_major: number | null;
	java_path: string | null;
	jvm_args: string[];
	jar: string;
	autostart: boolean;
	auto_restart: boolean;
	stop_timeout_secs: number;
	installed: boolean;
	created_at: number;
	status: Status;
	pid: number | null;
	started_at: number | null;
	uptime_secs: number | null;
	players: string[];
	cpu_percent: number;
	memory_bytes: number;
	restarts: number;
	last_error: string | null;
	last_exit: string | null;
	progress: string | null;
	dir: string;
}

export interface LogLine {
	seq: number;
	ts: number;
	stream: 'out' | 'err' | 'sys' | 'in';
	text: string;
}

export interface JavaRuntime {
	path: string;
	major: number;
	version: string;
	managed: boolean;
}

export interface SystemInfo {
	version: string;
	os: string;
	arch: string;
	data_dir: string;
	cpus: number;
	total_memory_bytes: number;
	available_memory_bytes: number;
}

const TOKEN_KEY = 'mcsd.token';

export const auth = {
	get token(): string | null {
		return localStorage.getItem(TOKEN_KEY);
	},
	set(token: string) {
		localStorage.setItem(TOKEN_KEY, token.trim());
	},
	clear() {
		localStorage.removeItem(TOKEN_KEY);
	}
};

/** `mcsd dashboard` prints a URL with `#token=...`: store it and clean the URL. */
export function captureTokenFromHash() {
	const m = location.hash.match(/token=([^&]+)/);
	if (m) {
		auth.set(decodeURIComponent(m[1]));
		history.replaceState(history.state, '', location.pathname + location.search);
	}
}

export class ApiError extends Error {
	status: number;
	constructor(status: number, message: string) {
		super(message);
		this.status = status;
	}
}

export async function api<T = unknown>(
	path: string,
	init: { method?: string; body?: unknown; token?: string } = {}
): Promise<T> {
	const headers: Record<string, string> = {
		Authorization: `Bearer ${init.token ?? auth.token ?? ''}`
	};
	if (init.body !== undefined) headers['Content-Type'] = 'application/json';
	const res = await fetch(`/api${path}`, {
		method: init.method ?? 'GET',
		headers,
		body: init.body !== undefined ? JSON.stringify(init.body) : undefined
	});
	if (res.status === 401 && !init.token) {
		auth.clear();
		goto('/login');
	}
	const text = await res.text();
	let data: any = undefined;
	try {
		data = text ? JSON.parse(text) : undefined;
	} catch {
		data = { error: text };
	}
	if (!res.ok) throw new ApiError(res.status, data?.error ?? res.statusText);
	return data as T;
}

export function consoleUrl(name: string): string {
	const proto = location.protocol === 'https:' ? 'wss' : 'ws';
	const token = encodeURIComponent(auth.token ?? '');
	return `${proto}://${location.host}/api/servers/${encodeURIComponent(name)}/console?token=${token}`;
}

export function fmtBytes(b: number): string {
	if (!b) return '–';
	const gb = b / 1024 ** 3;
	return gb >= 1 ? `${gb.toFixed(1)} GB` : `${Math.round(b / 1024 ** 2)} MB`;
}

export function fmtUptime(s: number | null): string {
	if (s == null) return '–';
	if (s < 60) return `${s}s`;
	if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
	if (s < 86400) return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
	return `${Math.floor(s / 86400)}d ${Math.floor((s % 86400) / 3600)}h`;
}

export const isActive = (s: Status) => s === 'starting' || s === 'running' || s === 'stopping';
