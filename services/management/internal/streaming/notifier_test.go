package streaming

import (
	"sync"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestSubscribeUnsubscribe(t *testing.T) {
	n := &Notifier{
		subscribers: make(map[uint64]chan<- Notification),
		done:        make(chan struct{}),
	}

	ch, unsub := n.Subscribe()
	require.NotNil(t, ch)

	n.mu.RLock()
	assert.Len(t, n.subscribers, 1)
	n.mu.RUnlock()

	unsub()

	n.mu.RLock()
	assert.Len(t, n.subscribers, 0)
	n.mu.RUnlock()
}

func TestFanOutToMultipleSubscribers(t *testing.T) {
	n := &Notifier{
		subscribers: make(map[uint64]chan<- Notification),
		done:        make(chan struct{}),
	}

	ch1, unsub1 := n.Subscribe()
	defer unsub1()
	ch2, unsub2 := n.Subscribe()
	defer unsub2()
	ch3, unsub3 := n.Subscribe()
	defer unsub3()

	notif := Notification{ExperimentID: "exp-1", Operation: "upsert"}
	n.fanOut(notif)

	for _, ch := range []<-chan Notification{ch1, ch2, ch3} {
		select {
		case got := <-ch:
			assert.Equal(t, "exp-1", got.ExperimentID)
			assert.Equal(t, "upsert", got.Operation)
		case <-time.After(time.Second):
			t.Fatal("timeout waiting for notification")
		}
	}
}

func TestSlowConsumerDoesNotBlock(t *testing.T) {
	n := &Notifier{
		subscribers: make(map[uint64]chan<- Notification),
		done:        make(chan struct{}),
	}

	// slow subscriber with tiny buffer
	slowCh := make(chan Notification, 1)
	n.mu.Lock()
	n.subscribers[0] = slowCh
	n.nextID = 1
	n.mu.Unlock()

	// fast subscriber
	fastCh, unsub := n.Subscribe()
	defer unsub()

	// Fill the slow subscriber's buffer
	slowCh <- Notification{ExperimentID: "fill", Operation: "upsert"}

	// This should not block even though slowCh is full
	done := make(chan struct{})
	go func() {
		n.fanOut(Notification{ExperimentID: "exp-2", Operation: "delete"})
		close(done)
	}()

	select {
	case <-done:
		// good — fanOut didn't block
	case <-time.After(time.Second):
		t.Fatal("fanOut blocked on slow subscriber")
	}

	// Fast subscriber should still receive it
	select {
	case got := <-fastCh:
		assert.Equal(t, "exp-2", got.ExperimentID)
	case <-time.After(time.Second):
		t.Fatal("fast subscriber didn't receive notification")
	}
}

func TestUnsubscribeCleansUpChannel(t *testing.T) {
	n := &Notifier{
		subscribers: make(map[uint64]chan<- Notification),
		done:        make(chan struct{}),
	}

	_, unsub1 := n.Subscribe()
	ch2, unsub2 := n.Subscribe()
	_, unsub3 := n.Subscribe()

	unsub1()
	unsub3()

	n.mu.RLock()
	assert.Len(t, n.subscribers, 1)
	n.mu.RUnlock()

	// Remaining subscriber still works
	n.fanOut(Notification{ExperimentID: "exp-3", Operation: "upsert"})
	select {
	case got := <-ch2:
		assert.Equal(t, "exp-3", got.ExperimentID)
	case <-time.After(time.Second):
		t.Fatal("remaining subscriber didn't receive notification")
	}

	unsub2()
	n.mu.RLock()
	assert.Len(t, n.subscribers, 0)
	n.mu.RUnlock()
}

func TestConcurrentSubscribeUnsubscribe(t *testing.T) {
	n := &Notifier{
		subscribers: make(map[uint64]chan<- Notification),
		done:        make(chan struct{}),
	}

	var wg sync.WaitGroup
	for i := 0; i < 50; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, unsub := n.Subscribe()
			// Simulate some usage
			n.fanOut(Notification{ExperimentID: "race", Operation: "upsert"})
			unsub()
		}()
	}
	wg.Wait()

	n.mu.RLock()
	assert.Len(t, n.subscribers, 0)
	n.mu.RUnlock()
}
