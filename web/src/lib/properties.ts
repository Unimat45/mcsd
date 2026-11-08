/** Typed metadata for the common server.properties keys. Keys not listed
 * here (newer versions, mod/plugin additions) still show up as plain text. */

export type PropType = 'bool' | 'int' | 'text' | 'enum' | 'secret';

export interface PropDef {
	key: string;
	label: string;
	type: PropType;
	default: string;
	help?: string;
	options?: string[];
	min?: number;
	max?: number;
}

export interface PropGroup {
	title: string;
	props: PropDef[];
}

export const PROPERTY_GROUPS: PropGroup[] = [
	{
		title: 'Gameplay',
		props: [
			{ key: 'motd', label: 'MOTD', type: 'text', default: 'A Minecraft Server', help: 'Shown in the server list. § color codes work.' },
			{ key: 'max-players', label: 'Max players', type: 'int', default: '20', min: 0 },
			{ key: 'gamemode', label: 'Default game mode', type: 'enum', default: 'survival', options: ['survival', 'creative', 'adventure', 'spectator'] },
			{ key: 'force-gamemode', label: 'Force game mode on join', type: 'bool', default: 'false' },
			{ key: 'difficulty', label: 'Difficulty', type: 'enum', default: 'easy', options: ['peaceful', 'easy', 'normal', 'hard'] },
			{ key: 'hardcore', label: 'Hardcore', type: 'bool', default: 'false' },
			{ key: 'pvp', label: 'PvP', type: 'bool', default: 'true' },
			{ key: 'allow-flight', label: 'Allow flight', type: 'bool', default: 'false', help: 'Stops players with flying mods from being kicked.' },
			{ key: 'spawn-protection', label: 'Spawn protection radius', type: 'int', default: '16', min: 0, help: '0 disables it. Ops are not affected.' },
			{ key: 'player-idle-timeout', label: 'Idle kick (minutes)', type: 'int', default: '0', min: 0, help: '0 disables it.' }
		]
	},
	{
		title: 'World',
		props: [
			{ key: 'level-name', label: 'World folder', type: 'text', default: 'world', help: 'Changing it loads (or creates) a different world.' },
			{ key: 'level-seed', label: 'Seed', type: 'text', default: '', help: 'Only used when a new world is generated.' },
			{ key: 'level-type', label: 'World type', type: 'enum', default: 'minecraft:normal', options: ['minecraft:normal', 'minecraft:flat', 'minecraft:large_biomes', 'minecraft:amplified', 'minecraft:single_biome_surface'], help: 'Only used when a new world is generated.' },
			{ key: 'generate-structures', label: 'Generate structures', type: 'bool', default: 'true' },
			{ key: 'allow-nether', label: 'Allow the Nether', type: 'bool', default: 'true' },
			{ key: 'view-distance', label: 'View distance (chunks)', type: 'int', default: '10', min: 3, max: 32 },
			{ key: 'simulation-distance', label: 'Simulation distance (chunks)', type: 'int', default: '10', min: 3, max: 32 },
			{ key: 'max-world-size', label: 'Max world radius (blocks)', type: 'int', default: '29999984', min: 1, max: 29999984 }
		]
	},
	{
		title: 'Access',
		props: [
			{ key: 'online-mode', label: 'Online mode', type: 'bool', default: 'true', help: 'Verifies accounts with Mojang. Turn off only behind a proxy like Velocity.' },
			{ key: 'white-list', label: 'Whitelist', type: 'bool', default: 'false' },
			{ key: 'enforce-whitelist', label: 'Kick non-whitelisted on reload', type: 'bool', default: 'false' },
			{ key: 'enforce-secure-profile', label: 'Require signed chat', type: 'bool', default: 'true' },
			{ key: 'op-permission-level', label: 'Op permission level', type: 'int', default: '4', min: 1, max: 4 },
			{ key: 'hide-online-players', label: 'Hide player list in server list', type: 'bool', default: 'false' },
			{ key: 'enable-status', label: 'Show in server list', type: 'bool', default: 'true' },
			{ key: 'prevent-proxy-connections', label: 'Block VPN/proxy connections', type: 'bool', default: 'false' }
		]
	},
	{
		title: 'Network & remote access',
		props: [
			{ key: 'server-port', label: 'Server port', type: 'int', default: '25565', min: 1024, max: 65535, help: 'Also updates the port mcsd tracks.' },
			{ key: 'server-ip', label: 'Bind address', type: 'text', default: '', help: 'Leave empty to listen on all interfaces.' },
			{ key: 'enable-rcon', label: 'Enable RCON', type: 'bool', default: 'false' },
			{ key: 'rcon.port', label: 'RCON port', type: 'int', default: '25575', min: 1, max: 65535 },
			{ key: 'rcon.password', label: 'RCON password', type: 'secret', default: '' },
			{ key: 'enable-query', label: 'Enable query (GameSpy4)', type: 'bool', default: 'false' },
			{ key: 'query.port', label: 'Query port', type: 'int', default: '25565', min: 1, max: 65535 },
			{ key: 'network-compression-threshold', label: 'Compression threshold (bytes)', type: 'int', default: '256', min: -1 },
			{ key: 'rate-limit', label: 'Packet rate limit', type: 'int', default: '0', min: 0, help: '0 disables it.' }
		]
	},
	{
		title: 'Advanced',
		props: [
			{ key: 'enable-command-block', label: 'Command blocks', type: 'bool', default: 'false' },
			{ key: 'function-permission-level', label: 'Function permission level', type: 'int', default: '2', min: 1, max: 4 },
			{ key: 'max-tick-time', label: 'Watchdog max tick time (ms)', type: 'int', default: '60000', min: -1, help: '-1 disables the watchdog.' },
			{ key: 'sync-chunk-writes', label: 'Synchronous chunk writes', type: 'bool', default: 'true' },
			{ key: 'entity-broadcast-range-percentage', label: 'Entity broadcast range (%)', type: 'int', default: '100', min: 10, max: 1000 },
			{ key: 'resource-pack', label: 'Resource pack URL', type: 'text', default: '' },
			{ key: 'resource-pack-sha1', label: 'Resource pack SHA-1', type: 'text', default: '' },
			{ key: 'require-resource-pack', label: 'Require resource pack', type: 'bool', default: 'false' },
			{ key: 'log-ips', label: 'Log player IPs', type: 'bool', default: 'true' }
		]
	}
];

export const KNOWN_KEYS = new Set(PROPERTY_GROUPS.flatMap((g) => g.props.map((p) => p.key)));

export function validate(def: PropDef | undefined, value: string): string | null {
	if (!def) return null;
	if (def.type === 'int') {
		if (!/^-?\d+$/.test(value.trim())) return 'Must be a whole number';
		const n = Number(value);
		if (def.min !== undefined && n < def.min) return `Minimum ${def.min}`;
		if (def.max !== undefined && n > def.max) return `Maximum ${def.max}`;
	}
	return null;
}
