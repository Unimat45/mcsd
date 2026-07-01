package provider

import (
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
)

// JavaInfo describes a detected Java runtime.
type JavaInfo struct {
	Path    string `json:"path"`
	Version string `json:"version"`
	Major   int    `json:"major"`
}

var javaVersionRe = regexp.MustCompile(`version "([^"]+)"`)

// JavaMajor runs `<path> -version` and parses the major version. It handles
// both the legacy "1.8.0_x" scheme (major 8) and the modern "21.0.1" scheme.
func JavaMajor(path string) (major int, version string, err error) {
	out, err := exec.Command(path, "-version").CombinedOutput()
	if err != nil {
		return 0, "", err
	}
	m := javaVersionRe.FindStringSubmatch(string(out))
	if m == nil {
		return 0, "", nil
	}
	version = m[1]
	segs := strings.FieldsFunc(version, func(r rune) bool { return r == '.' || r == '_' })
	if len(segs) == 0 {
		return 0, version, nil
	}
	first, _ := strconv.Atoi(segs[0])
	if first == 1 && len(segs) > 1 { // "1.8.0" -> 8
		second, _ := strconv.Atoi(segs[1])
		return second, version, nil
	}
	return first, version, nil
}

// DetectJavas finds Java runtimes on PATH, in $JAVA_HOME, and under common JVM
// install directories, deduplicated by resolved version.
func DetectJavas() []JavaInfo {
	var candidates []string
	if p, err := exec.LookPath("java"); err == nil {
		candidates = append(candidates, p)
	}
	if home := os.Getenv("JAVA_HOME"); home != "" {
		candidates = append(candidates, filepath.Join(home, "bin", "java"))
	}
	for _, base := range []string{"/usr/lib/jvm", "/usr/java", "/opt/java", "/Library/Java/JavaVirtualMachines"} {
		matches, _ := filepath.Glob(filepath.Join(base, "*", "bin", "java"))
		candidates = append(candidates, matches...)
		// macOS nests the binary deeper.
		deep, _ := filepath.Glob(filepath.Join(base, "*", "Contents", "Home", "bin", "java"))
		candidates = append(candidates, deep...)
	}

	var out []JavaInfo
	seenVersion := make(map[string]bool)
	seenPath := make(map[string]bool)
	for _, path := range candidates {
		resolved, err := filepath.EvalSymlinks(path)
		if err != nil {
			resolved = path
		}
		if seenPath[resolved] {
			continue
		}
		seenPath[resolved] = true

		major, version, err := JavaMajor(path)
		if err != nil || major == 0 || seenVersion[version] {
			continue
		}
		seenVersion[version] = true
		out = append(out, JavaInfo{Path: path, Version: version, Major: major})
	}
	return out
}
