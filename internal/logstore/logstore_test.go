package logstore

import (
	"fmt"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// TestRotationAndTail writes enough entries to force several rotations, then
// verifies Tail reads the most recent entries in order across the gzipped
// archives and the current file.
func TestRotationAndTail(t *testing.T) {
	dir := t.TempDir()
	// Small threshold so we rotate every few lines.
	s, err := New(dir, 256)
	if err != nil {
		t.Fatalf("New: %v", err)
	}

	const total = 200
	base := time.Unix(1_700_000_000, 0).UTC()
	for i := 0; i < total; i++ {
		s.Append(Entry{
			Ts:     base.Add(time.Duration(i) * time.Second),
			Stream: "stdout",
			Text:   fmt.Sprintf("line-%03d", i),
		})
	}
	s.Close() // flushes and finalizes

	// Reopen to exercise reading against existing files.
	s2, err := New(dir, 256)
	if err != nil {
		t.Fatalf("reopen: %v", err)
	}
	defer s2.Close()

	got, err := s2.Tail(10)
	if err != nil {
		t.Fatalf("Tail: %v", err)
	}
	if len(got) != 10 {
		t.Fatalf("Tail(10) returned %d entries, want 10", len(got))
	}
	for i, e := range got {
		want := fmt.Sprintf("line-%03d", total-10+i)
		if e.Text != want {
			t.Errorf("entry %d = %q, want %q", i, e.Text, want)
		}
	}

	// Confirm rotation actually produced gzipped archives.
	files, err := s2.orderedFiles()
	if err != nil {
		t.Fatalf("orderedFiles: %v", err)
	}
	gz := 0
	for _, f := range files {
		if strings.HasSuffix(f, ".gz") {
			gz++
		}
	}
	if gz == 0 {
		t.Errorf("expected at least one gzipped archive, got files: %v", files)
	}
	// The current file must sort last.
	if filepath.Base(files[len(files)-1]) != currentName {
		t.Errorf("current file not last in ordering: %v", files)
	}
}

// TestTailMoreThanExist returns everything when asked for more than present.
func TestTailMoreThanExist(t *testing.T) {
	dir := t.TempDir()
	s, err := New(dir, 0)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	for i := 0; i < 3; i++ {
		s.Append(Entry{Ts: time.Unix(int64(i), 0), Stream: "stdout", Text: fmt.Sprintf("l%d", i)})
	}
	s.Close()

	s2, _ := New(dir, 0)
	defer s2.Close()
	got, err := s2.Tail(100)
	if err != nil {
		t.Fatalf("Tail: %v", err)
	}
	if len(got) != 3 {
		t.Fatalf("got %d, want 3", len(got))
	}
}
