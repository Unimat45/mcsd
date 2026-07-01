package properties

// PropType classifies a property for UI rendering.
type PropType string

const (
	TypeBool   PropType = "bool"
	TypeInt    PropType = "int"
	TypeString PropType = "string"
	TypeEnum   PropType = "enum"
)

// Spec describes a known server.properties key.
type Spec struct {
	Key         string   `json:"key"`
	Type        PropType `json:"type"`
	Default     string   `json:"default"`
	Description string   `json:"description"`
	Options     []string `json:"options,omitempty"` // TypeEnum only
	Category    string   `json:"category"`
}

// Schema is a curated set of common Minecraft server.properties keys. Keys not
// listed here are still editable in the UI as free-text strings.
var Schema = []Spec{
	// --- General ---
	{Key: "motd", Type: TypeString, Default: "A Minecraft Server", Category: "General",
		Description: "Message shown in the server list."},
	{Key: "max-players", Type: TypeInt, Default: "20", Category: "General",
		Description: "Maximum number of simultaneous players."},
	{Key: "gamemode", Type: TypeEnum, Default: "survival", Category: "General",
		Options: []string{"survival", "creative", "adventure", "spectator"},
		Description: "Default game mode for new players."},
	{Key: "force-gamemode", Type: TypeBool, Default: "false", Category: "General",
		Description: "Force players to the default game mode on join."},
	{Key: "difficulty", Type: TypeEnum, Default: "easy", Category: "General",
		Options: []string{"peaceful", "easy", "normal", "hard"},
		Description: "World difficulty."},
	{Key: "hardcore", Type: TypeBool, Default: "false", Category: "General",
		Description: "Hardcore mode: players are banned on death."},
	{Key: "pvp", Type: TypeBool, Default: "true", Category: "General",
		Description: "Allow players to damage each other."},

	// --- Network ---
	{Key: "server-port", Type: TypeInt, Default: "25565", Category: "Network",
		Description: "TCP port the server listens on."},
	{Key: "server-ip", Type: TypeString, Default: "", Category: "Network",
		Description: "Bind address; empty binds all interfaces."},
	{Key: "online-mode", Type: TypeBool, Default: "true", Category: "Network",
		Description: "Authenticate players against Mojang. Disable only for LAN/offline."},
	{Key: "network-compression-threshold", Type: TypeInt, Default: "256", Category: "Network",
		Description: "Packet size above which to compress; -1 disables."},
	{Key: "prevent-proxy-connections", Type: TypeBool, Default: "false", Category: "Network",
		Description: "Kick players whose connection appears proxied."},

	// --- Access control ---
	{Key: "white-list", Type: TypeBool, Default: "false", Category: "Access",
		Description: "Enable the whitelist."},
	{Key: "enforce-whitelist", Type: TypeBool, Default: "false", Category: "Access",
		Description: "Kick non-whitelisted players when the whitelist reloads."},
	{Key: "op-permission-level", Type: TypeEnum, Default: "4", Category: "Access",
		Options: []string{"1", "2", "3", "4"},
		Description: "Permission level granted to operators."},
	{Key: "function-permission-level", Type: TypeEnum, Default: "2", Category: "Access",
		Options: []string{"1", "2", "3", "4"},
		Description: "Permission level required to run functions."},
	{Key: "player-idle-timeout", Type: TypeInt, Default: "0", Category: "Access",
		Description: "Minutes before idle players are kicked; 0 disables."},

	// --- World ---
	{Key: "level-name", Type: TypeString, Default: "world", Category: "World",
		Description: "World folder name."},
	{Key: "level-seed", Type: TypeString, Default: "", Category: "World",
		Description: "Seed for world generation; empty is random."},
	{Key: "level-type", Type: TypeEnum, Default: "minecraft:normal", Category: "World",
		Options: []string{"minecraft:normal", "minecraft:flat", "minecraft:large_biomes", "minecraft:amplified"},
		Description: "World generation preset."},
	{Key: "allow-nether", Type: TypeBool, Default: "true", Category: "World",
		Description: "Allow travel to the Nether."},
	{Key: "generate-structures", Type: TypeBool, Default: "true", Category: "World",
		Description: "Generate structures (villages, strongholds, etc.)."},
	{Key: "spawn-protection", Type: TypeInt, Default: "16", Category: "World",
		Description: "Radius (blocks) around spawn protected from non-ops; 0 disables."},
	{Key: "max-world-size", Type: TypeInt, Default: "29999984", Category: "World",
		Description: "World border radius in blocks."},

	// --- Gameplay / spawning ---
	{Key: "spawn-monsters", Type: TypeBool, Default: "true", Category: "Gameplay",
		Description: "Allow hostile mobs to spawn."},
	{Key: "allow-flight", Type: TypeBool, Default: "false", Category: "Gameplay",
		Description: "Allow flight (anti-cheat leniency for mods/plugins)."},
	{Key: "enable-command-block", Type: TypeBool, Default: "false", Category: "Gameplay",
		Description: "Enable command blocks."},

	// --- Performance ---
	{Key: "view-distance", Type: TypeInt, Default: "10", Category: "Performance",
		Description: "Chunk render distance sent to clients (2-32)."},
	{Key: "simulation-distance", Type: TypeInt, Default: "10", Category: "Performance",
		Description: "Chunk distance within which entities/ticking are simulated."},
	{Key: "max-tick-time", Type: TypeInt, Default: "60000", Category: "Performance",
		Description: "Watchdog: ms a single tick may take before the server is killed; -1 disables."},
	{Key: "sync-chunk-writes", Type: TypeBool, Default: "true", Category: "Performance",
		Description: "Write chunks synchronously (safer, slower)."},

	// --- Remote access ---
	{Key: "enable-rcon", Type: TypeBool, Default: "false", Category: "Remote",
		Description: "Enable the RCON remote console protocol."},
	{Key: "rcon.port", Type: TypeInt, Default: "25575", Category: "Remote",
		Description: "RCON listen port."},
	{Key: "rcon.password", Type: TypeString, Default: "", Category: "Remote",
		Description: "RCON password (required if RCON is enabled)."},
	{Key: "enable-query", Type: TypeBool, Default: "false", Category: "Remote",
		Description: "Enable the GameSpy4 query protocol."},
	{Key: "query.port", Type: TypeInt, Default: "25565", Category: "Remote",
		Description: "Query listen port."},
}

// specByKey indexes Schema for quick lookup.
var specByKey = func() map[string]Spec {
	m := make(map[string]Spec, len(Schema))
	for _, s := range Schema {
		m[s.Key] = s
	}
	return m
}()

// Lookup returns the spec for a key, if known.
func Lookup(key string) (Spec, bool) {
	s, ok := specByKey[key]
	return s, ok
}
