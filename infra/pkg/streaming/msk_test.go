package streaming

import (
	"testing"

	"github.com/kaizen-experimentation/infra/pkg/config"
)

func TestEnhancedMonitoring(t *testing.T) {
	tests := []struct {
		name string
		cfg  config.MskConfig
		want string
	}{
		{
			name: "prod always gets PER_TOPIC_PER_BROKER",
			cfg:  config.MskConfig{Environment: "prod"},
			want: "PER_TOPIC_PER_BROKER",
		},
		{
			name: "staging with explicit override",
			cfg:  config.MskConfig{Environment: "staging", EnhancedMonitoring: "PER_TOPIC_PER_PARTITION"},
			want: "PER_TOPIC_PER_PARTITION",
		},
		{
			name: "dev defaults to PER_BROKER",
			cfg:  config.MskConfig{Environment: "dev"},
			want: "PER_BROKER",
		},
		{
			name: "prod ignores override",
			cfg:  config.MskConfig{Environment: "prod", EnhancedMonitoring: "PER_BROKER"},
			want: "PER_TOPIC_PER_BROKER",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := enhancedMonitoring(tt.cfg)
			if got != tt.want {
				t.Errorf("enhancedMonitoring() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestLogRetentionDays(t *testing.T) {
	tests := []struct {
		env  string
		want int
	}{
		{"prod", 30},
		{"staging", 14},
		{"dev", 7},
		{"", 7},
	}

	for _, tt := range tests {
		t.Run(tt.env, func(t *testing.T) {
			got := logRetentionDays(tt.env)
			if got != tt.want {
				t.Errorf("logRetentionDays(%q) = %d, want %d", tt.env, got, tt.want)
			}
		})
	}
}
