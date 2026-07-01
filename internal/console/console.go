// Package console captures a child process's stdout/stderr line-by-line and
// lets callers write commands to its stdin. Each captured line is handed to an
// onLine callback; persistence and live fan-out are the caller's concern (see
// the supervisor's per-instance broker and log store).
package console

import (
	"bufio"
	"errors"
	"io"
	"sync"
	"time"
)

// Line is a single captured line of console output.
type Line struct {
	Ts     time.Time `json:"ts"`
	Stream string    `json:"stream"` // "stdout", "stderr", or "system"
	Text   string    `json:"text"`
}

// Console wraps one process's console I/O for the duration of a single launch.
type Console struct {
	mu     sync.Mutex
	stdin  io.WriteCloser
	closed bool
	onLine func(Line)

	wg sync.WaitGroup // tracks Attach reader goroutines
}

// New creates a Console over stdin. onLine is invoked for every captured line
// and may be called concurrently from the stdout and stderr readers, so it
// must be safe for concurrent use.
func New(stdin io.WriteCloser, onLine func(Line)) *Console {
	return &Console{stdin: stdin, onLine: onLine}
}

// Attach begins scanning r line-by-line, labeling each line with stream. It
// returns immediately; scanning runs in a goroutine until r hits EOF.
func (c *Console) Attach(stream string, r io.Reader) {
	c.wg.Add(1)
	go func() {
		defer c.wg.Done()
		sc := bufio.NewScanner(r)
		sc.Buffer(make([]byte, 0, 64*1024), 1024*1024) // tolerate long lines
		for sc.Scan() {
			c.emit(Line{Ts: time.Now(), Stream: stream, Text: sc.Text()})
		}
	}()
}

// Emit injects a synthetic "system" line (e.g. lifecycle notices).
func (c *Console) Emit(text string) {
	c.emit(Line{Ts: time.Now(), Stream: "system", Text: text})
}

func (c *Console) emit(l Line) {
	if c.onLine != nil {
		c.onLine(l)
	}
}

// WriteCommand writes a single command to the process's stdin, appending a
// newline. It errors if the console has been closed.
func (c *Console) WriteCommand(cmd string) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.closed || c.stdin == nil {
		return errors.New("console closed")
	}
	_, err := io.WriteString(c.stdin, cmd+"\n")
	return err
}

// Wait blocks until both attached streams reach EOF. This is the correct point
// to then call cmd.Wait() (all reads have completed).
func (c *Console) Wait() { c.wg.Wait() }

// Close closes stdin. Safe to call more than once.
func (c *Console) Close() {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.closed {
		return
	}
	c.closed = true
	if c.stdin != nil {
		_ = c.stdin.Close()
	}
}
