package provider

import (
	"strconv"
	"strings"
)

// parseVersion parses a release version like "1.20.4" into numeric components,
// ignoring any pre-release/snapshot suffix. ok is false for versions it can't
// parse (e.g. old snapshots like "23w13a").
func parseVersion(v string) (parts []int, ok bool) {
	base, _, _ := strings.Cut(v, "-")
	for _, seg := range strings.Split(base, ".") {
		n, err := strconv.Atoi(seg)
		if err != nil {
			return nil, false
		}
		parts = append(parts, n)
	}
	return parts, len(parts) > 0
}

// CompareVersions returns -1, 0, or 1. If either version is unparseable it
// returns 0 (treated as "can't tell").
func CompareVersions(a, b string) int {
	pa, oka := parseVersion(a)
	pb, okb := parseVersion(b)
	if !oka || !okb {
		return 0
	}
	for i := 0; i < len(pa) || i < len(pb); i++ {
		var x, y int
		if i < len(pa) {
			x = pa[i]
		}
		if i < len(pb) {
			y = pb[i]
		}
		if x != y {
			if x < y {
				return -1
			}
			return 1
		}
	}
	return 0
}

// IsDowngrade reports whether installing target over current is a version
// downgrade (world-corruption risk). Unparseable versions are treated as "not
// a known downgrade".
func IsDowngrade(target, current string) bool {
	return CompareVersions(target, current) < 0
}

// RecommendedJava returns the recommended major Java version for a Minecraft
// release. Best-effort; unknown/future versions map to the latest known (21).
func RecommendedJava(mcVersion string) int {
	parts, ok := parseVersion(mcVersion)
	if !ok {
		return 21
	}
	major := parts[0]
	minor, patch := 0, 0
	if len(parts) > 1 {
		minor = parts[1]
	}
	if len(parts) > 2 {
		patch = parts[2]
	}
	if major != 1 {
		return 21 // new-scheme / future versions
	}
	switch {
	case minor >= 21:
		return 21
	case minor == 20:
		if patch >= 5 {
			return 21
		}
		return 17
	case minor >= 18:
		return 17
	case minor == 17:
		return 16
	default:
		return 8
	}
}
