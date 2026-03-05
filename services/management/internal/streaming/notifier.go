// Package streaming provides PostgreSQL LISTEN/NOTIFY fan-out for experiment config changes.
package streaming

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"sync"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Notification represents a config change event.
type Notification struct {
	ExperimentID string `json:"experiment_id"`
	Operation    string `json:"operation"` // "upsert" or "delete"
}

const (
	channelName      = "experiment_config_changes"
	subscriberBuffer = 64
)

// Notifier listens on a PostgreSQL NOTIFY channel and fans out notifications
// to all registered subscribers.
type Notifier struct {
	pool        *pgxpool.Pool
	databaseURL string

	mu          sync.RWMutex
	subscribers map[uint64]chan<- Notification
	nextID      uint64

	cancel context.CancelFunc
	done   chan struct{}
}

// NewNotifier creates a new Notifier. Call Start() to begin listening.
func NewNotifier(pool *pgxpool.Pool, databaseURL string) *Notifier {
	return &Notifier{
		pool:        pool,
		databaseURL: databaseURL,
		subscribers: make(map[uint64]chan<- Notification),
		done:        make(chan struct{}),
	}
}

// Start spawns a goroutine that connects to PostgreSQL, issues LISTEN,
// and fans out notifications to subscribers.
func (n *Notifier) Start(ctx context.Context) {
	ctx, n.cancel = context.WithCancel(ctx)
	go n.listen(ctx)
}

func (n *Notifier) listen(ctx context.Context) {
	defer close(n.done)

	for {
		if err := n.listenLoop(ctx); err != nil {
			if ctx.Err() != nil {
				return
			}
			slog.Error("notifier: listen connection failed, retrying", "error", err)
			select {
			case <-ctx.Done():
				return
			case <-time.After(2 * time.Second):
			}
		}
	}
}

func (n *Notifier) listenLoop(ctx context.Context) error {
	conn, err := pgx.Connect(ctx, n.databaseURL)
	if err != nil {
		return fmt.Errorf("connect: %w", err)
	}
	defer conn.Close(ctx)

	if _, err := conn.Exec(ctx, "LISTEN "+channelName); err != nil {
		return fmt.Errorf("LISTEN: %w", err)
	}

	slog.Info("notifier: listening for config changes", "channel", channelName)

	for {
		notification, err := conn.WaitForNotification(ctx)
		if err != nil {
			return fmt.Errorf("wait: %w", err)
		}

		var notif Notification
		if err := json.Unmarshal([]byte(notification.Payload), &notif); err != nil {
			slog.Warn("notifier: invalid payload", "payload", notification.Payload, "error", err)
			continue
		}

		n.fanOut(notif)
	}
}

func (n *Notifier) fanOut(notif Notification) {
	n.mu.RLock()
	defer n.mu.RUnlock()

	for id, ch := range n.subscribers {
		select {
		case ch <- notif:
		default:
			slog.Warn("notifier: dropping notification for slow subscriber", "subscriber_id", id, "experiment_id", notif.ExperimentID)
		}
	}
}

// Subscribe registers a new subscriber. Returns a read-only channel for notifications
// and an unsubscribe function that must be called to clean up.
func (n *Notifier) Subscribe() (<-chan Notification, func()) {
	ch := make(chan Notification, subscriberBuffer)

	n.mu.Lock()
	id := n.nextID
	n.nextID++
	n.subscribers[id] = ch
	n.mu.Unlock()

	unsubscribe := func() {
		n.mu.Lock()
		delete(n.subscribers, id)
		n.mu.Unlock()
	}

	return ch, unsubscribe
}

// Publish sends a notification to the PostgreSQL NOTIFY channel.
// Called by handlers after successful mutations.
func (n *Notifier) Publish(ctx context.Context, experimentID, operation string) {
	payload, err := json.Marshal(Notification{
		ExperimentID: experimentID,
		Operation:    operation,
	})
	if err != nil {
		slog.Error("notifier: marshal payload", "error", err)
		return
	}

	if _, err := n.pool.Exec(ctx, "SELECT pg_notify($1, $2)", channelName, string(payload)); err != nil {
		slog.Error("notifier: publish failed", "error", err, "experiment_id", experimentID)
	}
}

// Stop cancels the listener goroutine and waits for it to finish.
func (n *Notifier) Stop() {
	if n.cancel != nil {
		n.cancel()
	}
	<-n.done
}
