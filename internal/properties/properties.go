// Package properties parses and round-trips Minecraft server.properties files,
// preserving comments, blank lines, and key order, and exposes a typed schema
// so the UI can render appropriate widgets.
package properties

import (
	"os"
	"path/filepath"
	"strings"
)

type lineKind int

const (
	kindKV lineKind = iota
	kindComment
	kindBlank
)

type line struct {
	kind  lineKind
	key   string // kindKV only
	value string // kindKV only
	raw   string // kindComment / kindBlank: verbatim text
}

// Properties is an ordered, comment-preserving view of a properties file.
type Properties struct {
	lines           []line
	index           map[string]int // key -> position in lines
	trailingNewline bool
}

// KV is a single key/value pair.
type KV struct {
	Key   string `json:"key"`
	Value string `json:"value"`
}

// Parse reads properties from bytes. It never fails: malformed lines are kept
// verbatim as comments so round-tripping is lossless.
func Parse(data []byte) *Properties {
	p := &Properties{index: make(map[string]int)}
	content := strings.ReplaceAll(string(data), "\r\n", "\n")
	content = strings.ReplaceAll(content, "\r", "\n")

	if strings.HasSuffix(content, "\n") {
		p.trailingNewline = true
		content = strings.TrimSuffix(content, "\n")
	}
	if content == "" && p.trailingNewline {
		// A file containing only a newline.
		return p
	}

	for _, raw := range strings.Split(content, "\n") {
		trimmed := strings.TrimSpace(raw)
		switch {
		case trimmed == "":
			p.lines = append(p.lines, line{kind: kindBlank, raw: raw})
		case strings.HasPrefix(trimmed, "#") || strings.HasPrefix(trimmed, "!"):
			p.lines = append(p.lines, line{kind: kindComment, raw: raw})
		default:
			key, value := splitKV(raw)
			p.index[key] = len(p.lines)
			p.lines = append(p.lines, line{kind: kindKV, key: key, value: value})
		}
	}
	return p
}

// splitKV splits on the first '=' (or ':' if no '=' present), trimming the key.
func splitKV(raw string) (key, value string) {
	sep := strings.IndexByte(raw, '=')
	if sep < 0 {
		if c := strings.IndexByte(raw, ':'); c >= 0 {
			sep = c
		}
	}
	if sep < 0 {
		return strings.TrimSpace(raw), ""
	}
	return strings.TrimSpace(raw[:sep]), raw[sep+1:]
}

// Get returns the value for key.
func (p *Properties) Get(key string) (string, bool) {
	if i, ok := p.index[key]; ok {
		return p.lines[i].value, true
	}
	return "", false
}

// Set updates an existing key in place or appends a new key at the end.
func (p *Properties) Set(key, value string) {
	if i, ok := p.index[key]; ok {
		p.lines[i].value = value
		return
	}
	p.index[key] = len(p.lines)
	p.lines = append(p.lines, line{kind: kindKV, key: key, value: value})
}

// Has reports whether key is present.
func (p *Properties) Has(key string) bool {
	_, ok := p.index[key]
	return ok
}

// Entries returns the key/value pairs in file order.
func (p *Properties) Entries() []KV {
	var out []KV
	for _, l := range p.lines {
		if l.kind == kindKV {
			out = append(out, KV{Key: l.key, Value: l.value})
		}
	}
	return out
}

// Map returns all key/value pairs.
func (p *Properties) Map() map[string]string {
	m := make(map[string]string, len(p.index))
	for _, l := range p.lines {
		if l.kind == kindKV {
			m[l.key] = l.value
		}
	}
	return m
}

// Order returns the keys in file order.
func (p *Properties) Order() []string {
	var out []string
	for _, l := range p.lines {
		if l.kind == kindKV {
			out = append(out, l.key)
		}
	}
	return out
}

// Bytes serializes back to the file format, preserving comments and order.
func (p *Properties) Bytes() []byte {
	var b strings.Builder
	for i, l := range p.lines {
		if i > 0 {
			b.WriteByte('\n')
		}
		if l.kind == kindKV {
			b.WriteString(l.key)
			b.WriteByte('=')
			b.WriteString(l.value)
		} else {
			b.WriteString(l.raw)
		}
	}
	if p.trailingNewline {
		b.WriteByte('\n')
	}
	return []byte(b.String())
}

// Load reads a properties file. A missing file yields an empty Properties and
// exists=false (not an error), so callers can edit a not-yet-created file.
func Load(path string) (p *Properties, exists bool, err error) {
	data, err := os.ReadFile(path)
	if os.IsNotExist(err) {
		empty := Parse(nil)
		empty.trailingNewline = true // new files end with a newline
		return empty, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	return Parse(data), true, nil
}

// Save atomically writes properties to path, creating parent dirs as needed.
func Save(path string, p *Properties) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, p.Bytes(), 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}
