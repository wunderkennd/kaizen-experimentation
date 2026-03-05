package allocation

import (
	"errors"
	"testing"
)

func TestFindContiguousGap(t *testing.T) {
	tests := []struct {
		name          string
		totalBuckets  int32
		occupied      []BucketRange
		bucketsNeeded int32
		wantStart     int32
		wantEnd       int32
		wantErr       error
	}{
		{
			name:          "empty layer",
			totalBuckets:  10000,
			occupied:      nil,
			bucketsNeeded: 5000,
			wantStart:     0,
			wantEnd:       4999,
		},
		{
			name:          "first half occupied",
			totalBuckets:  10000,
			occupied:      []BucketRange{{0, 4999}},
			bucketsNeeded: 5000,
			wantStart:     5000,
			wantEnd:       9999,
		},
		{
			name:          "second half occupied",
			totalBuckets:  10000,
			occupied:      []BucketRange{{5000, 9999}},
			bucketsNeeded: 5000,
			wantStart:     0,
			wantEnd:       4999,
		},
		{
			name:          "middle gap",
			totalBuckets:  10000,
			occupied:      []BucketRange{{0, 2499}, {7500, 9999}},
			bucketsNeeded: 3000,
			wantStart:     2500,
			wantEnd:       5499,
		},
		{
			name:          "exact fit at end",
			totalBuckets:  10000,
			occupied:      []BucketRange{{0, 4999}},
			bucketsNeeded: 5000,
			wantStart:     5000,
			wantEnd:       9999,
		},
		{
			name:          "insufficient capacity",
			totalBuckets:  10000,
			occupied:      []BucketRange{{0, 4999}, {5000, 9999}},
			bucketsNeeded: 1,
			wantErr:       ErrInsufficientCapacity,
		},
		{
			name:          "fragmented gaps - picks first fit",
			totalBuckets:  10000,
			occupied:      []BucketRange{{0, 999}, {3000, 5999}, {8000, 9999}},
			bucketsNeeded: 2000,
			wantStart:     1000,
			wantEnd:       2999,
		},
		{
			name:          "small request in large gap",
			totalBuckets:  10000,
			occupied:      []BucketRange{{0, 99}},
			bucketsNeeded: 1,
			wantStart:     100,
			wantEnd:       100,
		},
		{
			name:          "full layer single allocation",
			totalBuckets:  10000,
			occupied:      nil,
			bucketsNeeded: 10000,
			wantStart:     0,
			wantEnd:       9999,
		},
		{
			name:          "request exceeds total",
			totalBuckets:  10000,
			occupied:      nil,
			bucketsNeeded: 10001,
			wantErr:       ErrInsufficientCapacity,
		},
		{
			name:          "unsorted occupied ranges",
			totalBuckets:  10000,
			occupied:      []BucketRange{{5000, 9999}, {0, 2499}},
			bucketsNeeded: 2500,
			wantStart:     2500,
			wantEnd:       4999,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := FindContiguousGap(tt.totalBuckets, tt.occupied, tt.bucketsNeeded)
			if tt.wantErr != nil {
				if !errors.Is(err, tt.wantErr) {
					t.Fatalf("expected error %v, got %v", tt.wantErr, err)
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if got.Start != tt.wantStart || got.End != tt.wantEnd {
				t.Errorf("got [%d, %d], want [%d, %d]", got.Start, got.End, tt.wantStart, tt.wantEnd)
			}
		})
	}
}

func TestBucketsFromPercentage(t *testing.T) {
	tests := []struct {
		name         string
		totalBuckets int32
		percentage   float64
		want         int32
	}{
		{"100%", 10000, 1.0, 10000},
		{"50%", 10000, 0.5, 5000},
		{"10%", 10000, 0.1, 1000},
		{"1%", 10000, 0.01, 100},
		{"0.1%", 10000, 0.001, 10},
		{"tiny percentage rounds to 1", 10000, 0.00001, 1},
		{"zero percentage returns 1", 10000, 0.0, 1},
		{"negative percentage returns 1", 10000, -0.5, 1},
		{"over 100% clamped", 10000, 1.5, 10000},
		{"33.33%", 10000, 0.3333, 3333},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := BucketsFromPercentage(tt.totalBuckets, tt.percentage)
			if got != tt.want {
				t.Errorf("BucketsFromPercentage(%d, %f) = %d, want %d", tt.totalBuckets, tt.percentage, got, tt.want)
			}
		})
	}
}
