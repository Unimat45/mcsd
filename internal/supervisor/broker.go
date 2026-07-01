package supervisor

import (
	"sync"

	"github.com/mathieu/mcsd/internal/console"
)

// broker fans live console lines out to any number of subscribers. It lives on
// the Instance (not the per-launch Console) so a viewer's subscription
// survives server restarts.
type broker struct {
	mu   sync.Mutex
	subs map[int]chan console.Line
	next int
}

func newBroker() *broker {
	return &broker{subs: make(map[int]chan console.Line)}
}

// publish delivers l to every subscriber without blocking; a subscriber whose
// buffer is full drops the line rather than stalling console capture.
func (b *broker) publish(l console.Line) {
	b.mu.Lock()
	defer b.mu.Unlock()
	for _, ch := range b.subs {
		select {
		case ch <- l:
		default:
		}
	}
}

func (b *broker) subscribe() (int, <-chan console.Line) {
	b.mu.Lock()
	defer b.mu.Unlock()
	id := b.next
	b.next++
	ch := make(chan console.Line, 512)
	b.subs[id] = ch
	return id, ch
}

func (b *broker) unsubscribe(id int) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if ch, ok := b.subs[id]; ok {
		close(ch)
		delete(b.subs, id)
	}
}
