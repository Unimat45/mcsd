// Package logstore persists a server's console output to disk as JSON Lines,
// one entry per line, so history survives daemon and web-app restarts. The
// current file is rotated by size and gzipped, and Tail reads the most recent
// N entries across the current file and rotated archives.
//
// Writes are handled by a single background goroutine so callers (the console
// capture path) never block on disk I/O beyond channel backpressure.
package logstore

import (
	"bufio"
	"compress/gzip"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"
)

// Entry is one persisted console line.
type Entry struct {
	Ts     time.Time `json:"ts"`
	Stream string    `json:"stream"`
	Text   string    `json:"text"`
}

const (
	currentName       = "console.jsonl"
	defaultMaxBytes   = 10 * 1024 * 1024 // rotate at 10 MiB
	appendChannelSize = 4096
	rotateStampFormat = "20060102-150405.000000000" // lexicographically time-ordered
)

// Store is an append-only JSONL log for one server.
type Store struct {
	dir      string
	maxBytes int64

	ch     chan Entry
	closed chan struct{}
	done   chan struct{}
	once   sync.Once

	mu   sync.Mutex // guards f, w, size and file rotation
	f    *os.File
	w    *bufio.Writer
	size int64
}

// New opens (creating as needed) the log store in dir. maxBytes <= 0 uses the
// default rotation threshold.
func New(dir string, maxBytes int64) (*Store, error) {
	if maxBytes <= 0 {
		maxBytes = defaultMaxBytes
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, err
	}
	s := &Store{
		dir:      dir,
		maxBytes: maxBytes,
		ch:       make(chan Entry, appendChannelSize),
		closed:   make(chan struct{}),
		done:     make(chan struct{}),
	}
	if err := s.openCurrent(); err != nil {
		return nil, err
	}
	go s.loop()
	return s, nil
}

func (s *Store) openCurrent() error {
	path := filepath.Join(s.dir, currentName)
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
	if err != nil {
		return err
	}
	info, err := f.Stat()
	if err != nil {
		f.Close()
		return err
	}
	s.f = f
	s.w = bufio.NewWriter(f)
	s.size = info.Size()
	return nil
}

// Append queues an entry for persistence. It returns without blocking on disk;
// after Close it is a no-op.
func (s *Store) Append(e Entry) {
	select {
	case s.ch <- e:
	case <-s.closed:
	}
}

func (s *Store) loop() {
	defer close(s.done)
	for {
		select {
		case e := <-s.ch:
			s.write(e)
			if len(s.ch) == 0 { // batch: flush once the burst drains
				s.flush()
			}
		case <-s.closed:
			// Drain anything already queued, then flush and close.
			for {
				select {
				case e := <-s.ch:
					s.write(e)
				default:
					s.flush()
					s.closeFile()
					return
				}
			}
		}
	}
}

func (s *Store) write(e Entry) {
	b, err := json.Marshal(e)
	if err != nil {
		return
	}
	b = append(b, '\n')
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.w == nil {
		return
	}
	n, _ := s.w.Write(b)
	s.size += int64(n)
	if s.size >= s.maxBytes {
		if err := s.rotate(); err != nil {
			// Keep writing to the current file rather than losing output.
			fmt.Fprintf(os.Stderr, "logstore: rotate %s: %v\n", s.dir, err)
		}
	}
}

func (s *Store) flush() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.w != nil {
		_ = s.w.Flush()
	}
}

func (s *Store) closeFile() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.w != nil {
		_ = s.w.Flush()
	}
	if s.f != nil {
		_ = s.f.Close()
		s.f = nil
	}
}

// Reset deletes all persisted log files (current + rotated archives) and opens
// a fresh, empty current file. Used to clear a server's console history on a
// new run. Safe to call concurrently with the writer goroutine.
func (s *Store) Reset() error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if s.w != nil {
		_ = s.w.Flush()
	}
	if s.f != nil {
		_ = s.f.Close()
		s.f = nil
	}

	entries, err := os.ReadDir(s.dir)
	if err != nil {
		return err
	}
	for _, e := range entries {
		name := e.Name()
		if strings.HasPrefix(name, "console") &&
			(strings.HasSuffix(name, ".jsonl") || strings.HasSuffix(name, ".jsonl.gz")) {
			_ = os.Remove(filepath.Join(s.dir, name))
		}
	}
	return s.openCurrent()
}

// rotate is called with mu held. It flushes and closes the current file,
// renames it with a timestamp, gzips it, and opens a fresh current file.
func (s *Store) rotate() error {
	if s.w != nil {
		_ = s.w.Flush()
	}
	if s.f != nil {
		_ = s.f.Close()
	}
	current := filepath.Join(s.dir, currentName)
	stamp := time.Now().Format(rotateStampFormat)
	rotated := filepath.Join(s.dir, "console-"+stamp+".jsonl")
	if err := os.Rename(current, rotated); err != nil {
		// Reopen current so we don't lose the writer entirely.
		_ = s.openCurrent()
		return err
	}
	if err := gzipFile(rotated); err != nil {
		fmt.Fprintf(os.Stderr, "logstore: gzip %s: %v\n", rotated, err)
		// Leave the plain rotated file in place; Tail can still read it.
	}
	return s.openCurrent()
}

// Close flushes and stops the store. Safe to call once; subsequent Appends are
// dropped.
func (s *Store) Close() {
	s.once.Do(func() { close(s.closed) })
	<-s.done
}

// Tail returns the most recent n entries in chronological order, reading back
// through the current file and rotated archives as needed.
func (s *Store) Tail(n int) ([]Entry, error) {
	if n <= 0 {
		return nil, nil
	}
	s.flush()

	files, err := s.orderedFiles()
	if err != nil {
		return nil, err
	}

	// Walk newest -> oldest, collecting from the end of each file until we
	// have n entries.
	var collected []Entry
	for i := len(files) - 1; i >= 0 && len(collected) < n; i-- {
		entries, err := readEntries(files[i])
		if err != nil {
			continue // skip an unreadable/partial archive rather than fail
		}
		for j := len(entries) - 1; j >= 0 && len(collected) < n; j-- {
			collected = append(collected, entries[j])
		}
	}
	// collected is newest-first; reverse to chronological.
	for i, j := 0, len(collected)-1; i < j; i, j = i+1, j-1 {
		collected[i], collected[j] = collected[j], collected[i]
	}
	return collected, nil
}

// orderedFiles lists log files oldest -> newest (current file sorts last).
func (s *Store) orderedFiles() ([]string, error) {
	entries, err := os.ReadDir(s.dir)
	if err != nil {
		return nil, err
	}
	var names []string
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		name := e.Name()
		if strings.HasPrefix(name, "console") &&
			(strings.HasSuffix(name, ".jsonl") || strings.HasSuffix(name, ".jsonl.gz")) {
			names = append(names, name)
		}
	}
	sort.Strings(names) // timestamped archives < "console.jsonl"
	paths := make([]string, len(names))
	for i, n := range names {
		paths[i] = filepath.Join(s.dir, n)
	}
	return paths, nil
}

func readEntries(path string) ([]Entry, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()

	var r io.Reader = f
	if strings.HasSuffix(path, ".gz") {
		gr, err := gzip.NewReader(f)
		if err != nil {
			return nil, err
		}
		defer gr.Close()
		r = gr
	}

	sc := bufio.NewScanner(r)
	sc.Buffer(make([]byte, 0, 64*1024), 1024*1024)
	var out []Entry
	for sc.Scan() {
		var e Entry
		if json.Unmarshal(sc.Bytes(), &e) == nil {
			out = append(out, e)
		}
	}
	return out, sc.Err()
}

func gzipFile(path string) error {
	in, err := os.Open(path)
	if err != nil {
		return err
	}
	defer in.Close()

	out, err := os.Create(path + ".gz")
	if err != nil {
		return err
	}
	gw := gzip.NewWriter(out)
	if _, err := io.Copy(gw, in); err != nil {
		gw.Close()
		out.Close()
		return err
	}
	if err := gw.Close(); err != nil {
		out.Close()
		return err
	}
	if err := out.Close(); err != nil {
		return err
	}
	return os.Remove(path)
}
