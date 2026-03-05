package allocation

import (
	"errors"
	"math"
	"sort"
)

// ErrInsufficientCapacity is returned when the layer has no contiguous gap
// large enough for the requested number of buckets.
var ErrInsufficientCapacity = errors.New("insufficient bucket capacity in layer")

// BucketRange represents an inclusive range of hash buckets [Start, End].
type BucketRange struct {
	Start int32
	End   int32
}

// FindContiguousGap finds the first contiguous gap of at least bucketsNeeded
// buckets in a layer of totalBuckets, given the currently occupied ranges.
// Returns an inclusive [start, end] range. Uses first-fit search.
func FindContiguousGap(totalBuckets int32, occupied []BucketRange, bucketsNeeded int32) (BucketRange, error) {
	if bucketsNeeded <= 0 {
		return BucketRange{}, errors.New("bucketsNeeded must be positive")
	}
	if bucketsNeeded > totalBuckets {
		return BucketRange{}, ErrInsufficientCapacity
	}

	if len(occupied) == 0 {
		return BucketRange{Start: 0, End: bucketsNeeded - 1}, nil
	}

	// Sort occupied ranges by start bucket.
	sorted := make([]BucketRange, len(occupied))
	copy(sorted, occupied)
	sort.Slice(sorted, func(i, j int) bool {
		return sorted[i].Start < sorted[j].Start
	})

	// Check gap before first occupied range.
	if sorted[0].Start >= bucketsNeeded {
		return BucketRange{Start: 0, End: bucketsNeeded - 1}, nil
	}

	// Check gaps between occupied ranges.
	for i := 0; i < len(sorted)-1; i++ {
		gapStart := sorted[i].End + 1
		gapEnd := sorted[i+1].Start - 1
		gapSize := gapEnd - gapStart + 1
		if gapSize >= bucketsNeeded {
			return BucketRange{Start: gapStart, End: gapStart + bucketsNeeded - 1}, nil
		}
	}

	// Check gap after last occupied range.
	lastEnd := sorted[len(sorted)-1].End
	remaining := totalBuckets - 1 - lastEnd
	if remaining >= bucketsNeeded {
		return BucketRange{Start: lastEnd + 1, End: lastEnd + bucketsNeeded}, nil
	}

	return BucketRange{}, ErrInsufficientCapacity
}

// BucketsFromPercentage converts a traffic percentage (0.0–1.0) to a number
// of buckets. Rounds half-up, minimum 1.
func BucketsFromPercentage(totalBuckets int32, percentage float64) int32 {
	if percentage <= 0 {
		return 1
	}
	if percentage > 1 {
		percentage = 1
	}
	n := int32(math.Round(float64(totalBuckets) * percentage))
	if n < 1 {
		return 1
	}
	if n > totalBuckets {
		return totalBuckets
	}
	return n
}
