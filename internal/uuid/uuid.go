// Package uuid generates UUIDv7 identifiers (time-ordered, RFC 9562) using only
// the standard library.
package uuid

import (
	"crypto/rand"
	"encoding/hex"
	"time"
)

// NewV7 returns a new UUIDv7 as a canonical 36-char string. UUIDv7 embeds a
// millisecond timestamp in its high bits, so ids sort roughly by creation time.
func NewV7() string {
	var b [16]byte

	ms := time.Now().UnixMilli()
	b[0] = byte(ms >> 40)
	b[1] = byte(ms >> 32)
	b[2] = byte(ms >> 24)
	b[3] = byte(ms >> 16)
	b[4] = byte(ms >> 8)
	b[5] = byte(ms)

	if _, err := rand.Read(b[6:]); err != nil {
		// crypto/rand failing is not a recoverable condition.
		panic("uuid: crypto/rand read failed: " + err.Error())
	}
	b[6] = (b[6] & 0x0f) | 0x70 // version 7
	b[8] = (b[8] & 0x3f) | 0x80 // variant 10xxxxxx

	var s [36]byte
	hex.Encode(s[0:8], b[0:4])
	s[8] = '-'
	hex.Encode(s[9:13], b[4:6])
	s[13] = '-'
	hex.Encode(s[14:18], b[6:8])
	s[18] = '-'
	hex.Encode(s[19:23], b[8:10])
	s[23] = '-'
	hex.Encode(s[24:36], b[10:16])
	return string(s[:])
}
