// Command mcsctl is a thin CLI client for the mcsd daemon's HTTP API.
//
// Usage:
//
//	mcsctl [--addr host:port] <command> [args]
//
// Commands:
//
//	list                       list servers and their state
//	start   <id>               start a server
//	stop    <id>               gracefully stop a server
//	restart <id>               restart a server
//	send    <id> <command...>  send a console command to a running server
//	logs    <id> [--tail N] [--follow]   print recent logs, optionally follow
//	rm      <id>               remove a stopped server
//
// The daemon address defaults to $MCSD_ADDR or 127.0.0.1:8765.
package main

import (
	"bufio"
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
	"time"
)

const defaultAddr = "127.0.0.1:8765"

type client struct {
	base string
	http *http.Client
}

type statusDTO struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	State       string `json:"state"`
	AutoStart   bool   `json:"autostart"`
	AutoRestart bool   `json:"autorestart"`
}

type lineDTO struct {
	Ts     time.Time `json:"ts"`
	Stream string    `json:"stream"`
	Text   string    `json:"text"`
}

func main() {
	addr := os.Getenv("MCSD_ADDR")
	if addr == "" {
		addr = defaultAddr
	}

	args := os.Args[1:]
	// Accept a leading "--addr host:port".
	if len(args) >= 2 && args[0] == "--addr" {
		addr, args = args[1], args[2:]
	}
	if len(args) == 0 {
		usage()
		os.Exit(2)
	}

	c := &client{base: "http://" + addr, http: &http.Client{}}
	cmd, rest := args[0], args[1:]

	var err error
	switch cmd {
	case "list", "ls", "ps", "status":
		err = c.list()
	case "start":
		err = c.lifecycle("start", rest)
	case "stop":
		err = c.lifecycle("stop", rest)
	case "restart":
		err = c.lifecycle("restart", rest)
	case "send":
		err = c.send(rest)
	case "logs":
		err = c.logs(rest)
	case "rm", "remove", "delete":
		err = c.remove(rest)
	case "help", "-h", "--help":
		usage()
	default:
		fmt.Fprintf(os.Stderr, "unknown command %q\n\n", cmd)
		usage()
		os.Exit(2)
	}

	if err != nil {
		fmt.Fprintln(os.Stderr, "error:", err)
		os.Exit(1)
	}
}

func usage() {
	fmt.Fprint(os.Stderr, `mcsctl — control the mcsd daemon

usage: mcsctl [--addr host:port] <command> [args]

commands:
  list                        list servers and their state
  start   <id>                start a server
  stop    <id>                gracefully stop a server
  restart <id>                restart a server
  send    <id> <command...>   send a console command to a running server
  logs    <id> [--tail N] [--follow]
  rm      <id>                remove a stopped server
`)
}

func (c *client) list() error {
	var servers []statusDTO
	if err := c.getJSON("/api/servers", &servers); err != nil {
		return err
	}
	if len(servers) == 0 {
		fmt.Println("no servers configured")
		return nil
	}
	fmt.Printf("%-16s %-10s %-8s %s\n", "ID", "STATE", "AUTO", "NAME")
	for _, s := range servers {
		auto := ""
		if s.AutoStart {
			auto += "start"
		}
		if s.AutoRestart {
			if auto != "" {
				auto += ","
			}
			auto += "restart"
		}
		if auto == "" {
			auto = "-"
		}
		fmt.Printf("%-16s %-10s %-8s %s\n", s.ID, s.State, auto, s.Name)
	}
	return nil
}

func (c *client) lifecycle(action string, args []string) error {
	if len(args) != 1 {
		return fmt.Errorf("usage: mcsctl %s <id>", action)
	}
	var st statusDTO
	if err := c.postJSON("/api/servers/"+args[0]+"/"+action, nil, &st); err != nil {
		return err
	}
	fmt.Printf("%s: %s\n", st.ID, st.State)
	return nil
}

func (c *client) send(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: mcsctl send <id> <command...>")
	}
	id, command := args[0], strings.Join(args[1:], " ")
	body := map[string]string{"command": command}
	var resp map[string]string
	if err := c.postJSON("/api/servers/"+id+"/command", body, &resp); err != nil {
		return err
	}
	fmt.Printf("sent to %s: %s\n", id, command)
	return nil
}

func (c *client) remove(args []string) error {
	if len(args) != 1 {
		return fmt.Errorf("usage: mcsctl rm <id>")
	}
	req, _ := http.NewRequest(http.MethodDelete, c.base+"/api/servers/"+args[0], nil)
	resp, err := c.http.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 300 {
		return apiError(resp)
	}
	fmt.Printf("removed %s\n", args[0])
	return nil
}

func (c *client) logs(args []string) error {
	fs := flag.NewFlagSet("logs", flag.ContinueOnError)
	tail := fs.Int("tail", 200, "number of recent lines to show")
	follow := fs.Bool("follow", false, "stream new lines as they arrive")
	// The id is the first non-flag arg; split it out before parsing flags.
	if len(args) == 0 {
		return fmt.Errorf("usage: mcsctl logs <id> [--tail N] [--follow]")
	}
	id := args[0]
	if err := fs.Parse(args[1:]); err != nil {
		return err
	}

	if !*follow {
		var lines []lineDTO
		if err := c.getJSON(fmt.Sprintf("/api/servers/%s/logs?tail=%d", id, *tail), &lines); err != nil {
			return err
		}
		for _, l := range lines {
			fmt.Println(formatLine(l))
		}
		return nil
	}

	// Follow: read the SSE stream until interrupted.
	resp, err := c.http.Get(fmt.Sprintf("%s/api/servers/%s/stream?tail=%d", c.base, id, *tail))
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 300 {
		return apiError(resp)
	}
	sc := bufio.NewScanner(resp.Body)
	sc.Buffer(make([]byte, 0, 64*1024), 1024*1024)
	for sc.Scan() {
		text := sc.Text()
		data, ok := strings.CutPrefix(text, "data: ")
		if !ok {
			continue // skip comments/pings and blank separators
		}
		var l lineDTO
		if json.Unmarshal([]byte(data), &l) == nil {
			fmt.Println(formatLine(l))
		}
	}
	return sc.Err()
}

func formatLine(l lineDTO) string {
	switch l.Stream {
	case "stderr":
		return "! " + l.Text
	case "system":
		return "* " + l.Text
	default:
		return l.Text
	}
}

// --- HTTP helpers ---

func (c *client) getJSON(path string, out any) error {
	resp, err := c.http.Get(c.base + path)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 300 {
		return apiError(resp)
	}
	return json.NewDecoder(resp.Body).Decode(out)
}

func (c *client) postJSON(path string, body, out any) error {
	var buf io.Reader
	if body != nil {
		b, err := json.Marshal(body)
		if err != nil {
			return err
		}
		buf = bytes.NewReader(b)
	}
	resp, err := c.http.Post(c.base+path, "application/json", buf)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 300 {
		return apiError(resp)
	}
	if out != nil {
		return json.NewDecoder(resp.Body).Decode(out)
	}
	return nil
}

func apiError(resp *http.Response) error {
	var e struct {
		Error string `json:"error"`
	}
	b, _ := io.ReadAll(resp.Body)
	if json.Unmarshal(b, &e) == nil && e.Error != "" {
		return fmt.Errorf("%s: %s", resp.Status, e.Error)
	}
	return fmt.Errorf("%s: %s", resp.Status, strings.TrimSpace(string(b)))
}
