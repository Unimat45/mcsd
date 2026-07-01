package properties

import "testing"

func TestRoundTripPreservesCommentsAndOrder(t *testing.T) {
	src := "#Minecraft server properties\n#Mon Jul 01 00:00:00 UTC 2026\nmotd=Hello World\n\ndifficulty=easy\nmax-players=20\n"
	p := Parse([]byte(src))
	if got := string(p.Bytes()); got != src {
		t.Fatalf("round-trip changed content:\n--- got ---\n%q\n--- want ---\n%q", got, src)
	}
}

func TestSetInPlaceAndAppend(t *testing.T) {
	src := "#header\nmotd=old\ndifficulty=easy\n"
	p := Parse([]byte(src))

	p.Set("motd", "new")          // update existing, in place
	p.Set("max-players", "50")    // append new key

	want := "#header\nmotd=new\ndifficulty=easy\nmax-players=50\n"
	if got := string(p.Bytes()); got != want {
		t.Fatalf("got %q, want %q", got, want)
	}

	if v, _ := p.Get("motd"); v != "new" {
		t.Errorf("Get(motd) = %q, want new", v)
	}
	if order := p.Order(); len(order) != 3 || order[0] != "motd" || order[2] != "max-players" {
		t.Errorf("unexpected order: %v", order)
	}
}

func TestValuePreservesSpacesAndEquals(t *testing.T) {
	// Values may contain '=' (e.g. base64) and leading/trailing spaces.
	src := "resource-pack=https://x/y?a=b&c=d\nmotd= spaced value \n"
	p := Parse([]byte(src))
	if v, _ := p.Get("resource-pack"); v != "https://x/y?a=b&c=d" {
		t.Errorf("value with '=' mangled: %q", v)
	}
	if v, _ := p.Get("motd"); v != " spaced value " {
		t.Errorf("value spaces not preserved: %q", v)
	}
	if got := string(p.Bytes()); got != src {
		t.Errorf("round-trip mismatch: %q", got)
	}
}

func TestSchemaLookup(t *testing.T) {
	if s, ok := Lookup("difficulty"); !ok || s.Type != TypeEnum || len(s.Options) != 4 {
		t.Errorf("difficulty spec wrong: %+v ok=%v", s, ok)
	}
	if _, ok := Lookup("not-a-real-key"); ok {
		t.Errorf("unexpected spec for unknown key")
	}
}
