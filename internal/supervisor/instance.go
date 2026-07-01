package supervisor

import (
	"context"
	"fmt"
	"log"
	"os/exec"
	"sync"
	"syscall"
	"time"

	"github.com/mathieu/mcsd/internal/config"
	"github.com/mathieu/mcsd/internal/console"
	"github.com/mathieu/mcsd/internal/logstore"
)

// State is a server's lifecycle state.
type State string

const (
	StateStopped  State = "stopped"  // not running, no restart pending
	StateStarting State = "starting" // process launched, not yet confirmed up
	StateRunning  State = "running"  // process is up
	StateStopping State = "stopping" // graceful shutdown in progress
	StateCrashed  State = "crashed"  // exited unexpectedly; may be restarting
	StateErrored  State = "errored"  // gave up after repeated crashes
)

const (
	defaultStopTimeout = 30 * time.Second
	sigtermGrace       = 10 * time.Second // wait after SIGTERM before SIGKILL
	backoffBase        = 1 * time.Second
	backoffMax         = 60 * time.Second
	crashWindow        = 60 * time.Second // window for crash-loop detection
	crashThreshold     = 5                // crashes within window => errored
)

// Instance is the runtime supervisor for one configured server. A single
// supervise goroutine owns launching and restart decisions; external callers
// drive it via Start/Stop/Restart.
type Instance struct {
	id     string // immutable server id
	logger *log.Logger
	store  *logstore.Store // persistent JSONL console history; may be nil
	broker *broker         // live line fan-out; survives across launches

	mu       sync.Mutex
	cfg      config.ServerConfig
	state    State
	desired  State // StateRunning or StateStopped — what we want the loop to do
	cmd      *exec.Cmd
	con      *console.Console
	exited   chan struct{} // closed when the current process's Wait returns
	restarts []time.Time   // crash timestamps within the sliding window

	// loop bookkeeping, guarded by loopMu
	loopMu     sync.Mutex
	running    bool
	loopDone   chan struct{}
	loopCancel context.CancelFunc
}

// NewInstance creates a supervisor for cfg in the stopped state. logDir is
// where the persistent console log is written; if the store can't be opened
// the instance still runs, just without on-disk history.
func NewInstance(cfg config.ServerConfig, logger *log.Logger, logDir string) *Instance {
	store, err := logstore.New(logDir, 0)
	if err != nil {
		logger.Printf("[%s] log store unavailable (%v); running without persistence", cfg.ID, err)
		store = nil
	}
	return &Instance{
		id:      cfg.ID,
		cfg:     cfg,
		logger:  logger,
		store:   store,
		broker:  newBroker(),
		state:   StateStopped,
		desired: StateStopped,
	}
}

// ID returns the immutable server id.
func (i *Instance) ID() string { return i.id }

// Config returns a copy of the server's current configuration.
func (i *Instance) Config() config.ServerConfig {
	i.mu.Lock()
	defer i.mu.Unlock()
	return i.cfg
}

// UpdateConfig replaces the server's configuration. Changes to launch
// parameters (jar, java args, etc.) take effect on the next start. The id and
// working directory are daemon-managed and preserved regardless of cfg.
func (i *Instance) UpdateConfig(cfg config.ServerConfig) {
	i.mu.Lock()
	cfg.ID = i.id
	cfg.Directory = i.cfg.Directory
	i.cfg = cfg
	i.mu.Unlock()
}

// Subscribe registers a live line listener that persists across process
// restarts. Pair with Unsubscribe.
func (i *Instance) Subscribe() (int, <-chan console.Line) { return i.broker.subscribe() }

// Unsubscribe removes a live listener.
func (i *Instance) Unsubscribe(id int) { i.broker.unsubscribe(id) }

// State returns the current lifecycle state.
func (i *Instance) State() State {
	i.mu.Lock()
	defer i.mu.Unlock()
	return i.state
}

// Console returns the live console for the current process, or nil if not
// running.
func (i *Instance) Console() *console.Console {
	i.mu.Lock()
	defer i.mu.Unlock()
	return i.con
}

// Send writes a command to the running server's stdin.
func (i *Instance) Send(cmd string) error {
	con := i.Console()
	if con == nil {
		return fmt.Errorf("server %q is not running", i.id)
	}
	return con.WriteCommand(cmd)
}

// clearLogs wipes the persisted console history and tells live viewers to
// reset their display, so a new run starts with a clean console.
func (i *Instance) clearLogs() {
	if i.store != nil {
		if err := i.store.Reset(); err != nil {
			i.logf("failed to clear logs: %v", err)
		}
	}
	// Broadcast a non-persisted reset marker to connected consoles.
	i.broker.publish(console.Line{Ts: time.Now(), Stream: "reset"})
}

// Start ensures the supervise loop is running and drives the server toward the
// running state. It is a no-op if the loop is already active.
func (i *Instance) Start() {
	i.loopMu.Lock()
	defer i.loopMu.Unlock()
	if i.running {
		i.setDesired(StateRunning)
		return
	}
	// Fresh run: clear the persisted console history. This runs only on a
	// user-initiated start (and restart, which calls Start) — NOT on automatic
	// crash-restarts inside the supervise loop, so crash output is preserved.
	i.clearLogs()

	i.setDesired(StateRunning)
	i.setState(StateStarting) // reflect intent immediately for API/CLI responses
	ctx, cancel := context.WithCancel(context.Background())
	i.loopCancel = cancel
	i.loopDone = make(chan struct{})
	i.running = true
	go i.supervise(ctx)
}

// Stop gracefully stops the server and prevents further restarts. It blocks
// until the process is down and the supervise loop has exited.
func (i *Instance) Stop() {
	i.setDesired(StateStopped)

	i.mu.Lock()
	cmd, con, exited := i.cmd, i.con, i.exited
	i.mu.Unlock()

	if cmd != nil && cmd.Process != nil {
		i.stopProcess(cmd, con, exited)
	}
	i.cancelLoop() // interrupt any backoff sleep
	i.waitLoop()
}

// Restart performs a graceful stop followed by a start.
func (i *Instance) Restart() {
	i.Stop()
	i.Start()
}

// stopProcess sends "stop" then escalates through SIGTERM and SIGKILL until the
// process exits.
func (i *Instance) stopProcess(cmd *exec.Cmd, con *console.Console, exited chan struct{}) {
	i.setState(StateStopping)
	timeout := time.Duration(i.Config().StopTimeoutSeconds) * time.Second
	if timeout <= 0 {
		timeout = defaultStopTimeout
	}

	if con != nil {
		i.logf("sending 'stop' command")
		_ = con.WriteCommand("stop")
	}
	if waitFor(exited, timeout) {
		return
	}
	i.logf("stop timed out after %s; sending SIGTERM", timeout)
	_ = cmd.Process.Signal(syscall.SIGTERM)
	if waitFor(exited, sigtermGrace) {
		return
	}
	i.logf("SIGTERM ignored; sending SIGKILL")
	_ = cmd.Process.Kill()
	<-exited
}

// supervise is the single owner of process launches and restart policy.
func (i *Instance) supervise(ctx context.Context) {
	defer func() {
		i.loopMu.Lock()
		i.running = false
		close(i.loopDone)
		i.loopMu.Unlock()
	}()

	for {
		if i.getDesired() == StateStopped || ctx.Err() != nil {
			i.setState(StateStopped)
			return
		}

		err := i.launch(ctx)

		if i.getDesired() == StateStopped {
			i.setState(StateStopped)
			return
		}

		// Unexpected exit.
		i.logf("process exited unexpectedly: %v", err)
		if !i.Config().AutoRestart {
			i.setState(StateCrashed)
			return
		}

		i.recordCrash()
		if i.crashLooping() {
			i.logf("crash loop detected (%d crashes within %s); giving up", crashThreshold, crashWindow)
			i.broker.publish(console.Line{Ts: time.Now(), Stream: "system", Text: "[mcsd] crash loop detected; giving up"})
			i.setState(StateErrored)
			return
		}

		delay := i.backoff()
		i.setState(StateCrashed)
		i.logf("restarting in %s", delay)
		select {
		case <-time.After(delay):
		case <-ctx.Done():
			i.setState(StateStopped)
			return
		}
	}
}

// launch starts one process and blocks until it exits. The returned error is
// the process's exit status (nil on clean exit).
func (i *Instance) launch(ctx context.Context) error {
	i.setState(StateStarting)

	cfg := i.Config() // snapshot; config may be updated between launches
	javaPath := cfg.JavaPath
	if javaPath == "" {
		javaPath = "java"
	}
	var args []string
	args = append(args, cfg.JavaArgs...)
	if cfg.Jar != "" {
		args = append(args, "-jar", cfg.Jar)
	}
	args = append(args, cfg.ServerArgs...)

	cmd := exec.Command(javaPath, args...)
	cmd.Dir = cfg.Directory

	stdin, err := cmd.StdinPipe()
	if err != nil {
		return fmt.Errorf("stdin pipe: %w", err)
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return fmt.Errorf("stdout pipe: %w", err)
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		return fmt.Errorf("stderr pipe: %w", err)
	}

	con := console.New(stdin, i.onLine)
	con.Attach("stdout", stdout)
	con.Attach("stderr", stderr)

	if err := cmd.Start(); err != nil {
		con.Close()
		return fmt.Errorf("start: %w", err)
	}

	exited := make(chan struct{})
	i.mu.Lock()
	i.cmd = cmd
	i.con = con
	i.exited = exited
	i.mu.Unlock()

	i.setState(StateRunning)
	i.logf("started pid=%d (%s)", cmd.Process.Pid, javaPath)
	con.Emit(fmt.Sprintf("[mcsd] started pid=%d", cmd.Process.Pid))

	// Drain both pipes to EOF before Wait (required by os/exec), then reap.
	con.Wait()
	waitErr := cmd.Wait()
	close(exited)
	con.Close()

	i.mu.Lock()
	i.cmd = nil
	i.con = nil
	i.exited = nil
	i.mu.Unlock()

	return waitErr
}

// onLine handles each captured console line: it persists the line to the JSONL
// store (so history survives restarts) and publishes it to live subscribers.
// It is safe for concurrent use.
func (i *Instance) onLine(l console.Line) {
	if i.store != nil {
		i.store.Append(logstore.Entry{Ts: l.Ts, Stream: l.Stream, Text: l.Text})
	}
	i.broker.publish(l)
}

// Tail returns the most recent n persisted console lines in chronological
// order — the history a freshly (re)connected viewer should see.
func (i *Instance) Tail(n int) ([]logstore.Entry, error) {
	if i.store == nil {
		return nil, nil
	}
	return i.store.Tail(n)
}

// CloseStore flushes and closes the persistent log. Call on daemon shutdown,
// after the process has stopped.
func (i *Instance) CloseStore() {
	if i.store != nil {
		i.store.Close()
	}
}

// --- restart policy helpers ---

func (i *Instance) recordCrash() {
	i.mu.Lock()
	defer i.mu.Unlock()
	now := time.Now()
	cutoff := now.Add(-crashWindow)
	kept := i.restarts[:0]
	for _, t := range i.restarts {
		if t.After(cutoff) {
			kept = append(kept, t)
		}
	}
	i.restarts = append(kept, now)
}

func (i *Instance) crashLooping() bool {
	i.mu.Lock()
	defer i.mu.Unlock()
	return len(i.restarts) >= crashThreshold
}

func (i *Instance) backoff() time.Duration {
	i.mu.Lock()
	n := len(i.restarts)
	i.mu.Unlock()
	if n < 1 {
		n = 1
	}
	d := backoffBase << (n - 1) // 1s, 2s, 4s, ...
	if d <= 0 || d > backoffMax {
		return backoffMax
	}
	return d
}

// --- small state/loop helpers ---

func (i *Instance) setState(s State) {
	i.mu.Lock()
	prev := i.state
	i.state = s
	i.mu.Unlock()
	if prev != s {
		i.logf("state %s -> %s", prev, s)
	}
}

func (i *Instance) getDesired() State {
	i.mu.Lock()
	defer i.mu.Unlock()
	return i.desired
}

func (i *Instance) setDesired(s State) {
	i.mu.Lock()
	i.desired = s
	i.mu.Unlock()
}

func (i *Instance) cancelLoop() {
	i.loopMu.Lock()
	cancel := i.loopCancel
	i.loopMu.Unlock()
	if cancel != nil {
		cancel()
	}
}

func (i *Instance) waitLoop() {
	i.loopMu.Lock()
	done := i.loopDone
	i.loopMu.Unlock()
	if done != nil {
		<-done
	}
}

func (i *Instance) logf(format string, args ...any) {
	i.logger.Printf("[%s] "+format, append([]any{i.id}, args...)...)
}

// waitFor reports whether ch closed within d.
func waitFor(ch <-chan struct{}, d time.Duration) bool {
	if ch == nil {
		return true
	}
	select {
	case <-ch:
		return true
	case <-time.After(d):
		return false
	}
}
