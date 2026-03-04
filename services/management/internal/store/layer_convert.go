package store

import (
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	"google.golang.org/protobuf/types/known/durationpb"
	"google.golang.org/protobuf/types/known/timestamppb"
	"time"
)

// LayerRowToProto converts a LayerRow to a proto Layer.
func LayerRowToProto(row LayerRow) *commonv1.Layer {
	return &commonv1.Layer{
		LayerId:              row.LayerID,
		Name:                 row.Name,
		Description:          row.Description,
		TotalBuckets:         row.TotalBuckets,
		BucketReuseCooldown: durationpb.New(time.Duration(row.BucketReuseCooldownSeconds) * time.Second),
	}
}

// LayerProtoToRow converts a proto Layer to a LayerRow with defaults.
func LayerProtoToRow(l *commonv1.Layer) LayerRow {
	row := LayerRow{
		LayerID:     l.GetLayerId(),
		Name:        l.GetName(),
		Description: l.GetDescription(),
		TotalBuckets: l.GetTotalBuckets(),
	}

	if row.TotalBuckets <= 0 {
		row.TotalBuckets = 10000
	}

	if cd := l.GetBucketReuseCooldown(); cd != nil {
		row.BucketReuseCooldownSeconds = int32(cd.GetSeconds())
	} else {
		row.BucketReuseCooldownSeconds = 86400 // 24 hours
	}

	return row
}

// AllocationRowToProto converts an AllocationRow to a proto LayerAllocation.
func AllocationRowToProto(row AllocationRow) *commonv1.LayerAllocation {
	a := &commonv1.LayerAllocation{
		AllocationId: row.AllocationID,
		LayerId:      row.LayerID,
		ExperimentId: row.ExperimentID,
		StartBucket:  row.StartBucket,
		EndBucket:    row.EndBucket,
	}
	if row.ActivatedAt != nil {
		a.ActivatedAt = timestamppb.New(*row.ActivatedAt)
	}
	if row.ReleasedAt != nil {
		a.ReleasedAt = timestamppb.New(*row.ReleasedAt)
	}
	if row.ReusableAfter != nil {
		a.ReusableAfter = timestamppb.New(*row.ReusableAfter)
	}
	return a
}
