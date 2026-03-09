package handlers

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"math"
	"strings"

	"connectrpc.com/connect"
	"github.com/org/experimentation-platform/services/flags/internal/store"
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"
)

func (s *FlagService) CreateFlag(ctx context.Context, req *connect.Request[flagsv1.CreateFlagRequest]) (*connect.Response[flagsv1.Flag], error) {
	pb := req.Msg.GetFlag()
	if pb == nil {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("flag is required"))
	}

	if err := validateFlag(pb); err != nil {
		return nil, connect.NewError(connect.CodeInvalidArgument, err)
	}

	f := protoToFlag(pb)
	created, err := s.store.CreateFlag(ctx, f)
	if err != nil {
		if strings.Contains(err.Error(), "already exists") {
			return nil, connect.NewError(connect.CodeAlreadyExists, err)
		}
		return nil, connect.NewError(connect.CodeInternal, fmt.Errorf("create flag: %w", err))
	}

	s.recordAudit(ctx, created.FlagID, "create", nil, created)

	return connect.NewResponse(flagToProto(created)), nil
}

func (s *FlagService) GetFlag(ctx context.Context, req *connect.Request[flagsv1.GetFlagRequest]) (*connect.Response[flagsv1.Flag], error) {
	flagID := req.Msg.GetFlagId()
	if flagID == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("flag_id is required"))
	}

	f, err := s.store.GetFlag(ctx, flagID)
	if err != nil {
		if strings.Contains(err.Error(), "not found") {
			return nil, connect.NewError(connect.CodeNotFound, err)
		}
		return nil, connect.NewError(connect.CodeInternal, fmt.Errorf("get flag: %w", err))
	}

	return connect.NewResponse(flagToProto(f)), nil
}

func (s *FlagService) UpdateFlag(ctx context.Context, req *connect.Request[flagsv1.UpdateFlagRequest]) (*connect.Response[flagsv1.Flag], error) {
	pb := req.Msg.GetFlag()
	if pb == nil {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("flag is required"))
	}
	if pb.GetFlagId() == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("flag_id is required"))
	}

	if err := validateFlag(pb); err != nil {
		return nil, connect.NewError(connect.CodeInvalidArgument, err)
	}

	// Fetch previous state for audit trail.
	var previous *store.Flag
	if s.auditStore != nil {
		previous, _ = s.store.GetFlag(ctx, pb.GetFlagId())
	}

	f := protoToFlag(pb)
	updated, err := s.store.UpdateFlag(ctx, f)
	if err != nil {
		if strings.Contains(err.Error(), "not found") {
			return nil, connect.NewError(connect.CodeNotFound, err)
		}
		if strings.Contains(err.Error(), "already exists") {
			return nil, connect.NewError(connect.CodeAlreadyExists, err)
		}
		return nil, connect.NewError(connect.CodeInternal, fmt.Errorf("update flag: %w", err))
	}

	// Determine specific action for audit.
	action := "update"
	if previous != nil {
		if previous.Enabled != updated.Enabled {
			if updated.Enabled {
				action = "enable"
			} else {
				action = "disable"
			}
		} else if previous.RolloutPercentage != updated.RolloutPercentage {
			action = "rollout_change"
		}
	}
	s.recordAudit(ctx, updated.FlagID, action, previous, updated)

	return connect.NewResponse(flagToProto(updated)), nil
}

func (s *FlagService) ListFlags(ctx context.Context, req *connect.Request[flagsv1.ListFlagsRequest]) (*connect.Response[flagsv1.ListFlagsResponse], error) {
	pageSize := int(req.Msg.GetPageSize())
	pageToken := req.Msg.GetPageToken()

	flags, nextToken, err := s.store.ListFlags(ctx, pageSize, pageToken)
	if err != nil {
		return nil, connect.NewError(connect.CodeInternal, fmt.Errorf("list flags: %w", err))
	}

	resp := &flagsv1.ListFlagsResponse{
		NextPageToken: nextToken,
	}
	for _, f := range flags {
		resp.Flags = append(resp.Flags, flagToProto(f))
	}

	return connect.NewResponse(resp), nil
}

// recordAudit writes an audit entry if an audit store is configured.
// Errors are logged but do not fail the operation.
func (s *FlagService) recordAudit(ctx context.Context, flagID, action string, previous, current *store.Flag) {
	if s.auditStore == nil {
		return
	}

	entry := &store.AuditEntry{
		FlagID:     flagID,
		Action:     action,
		ActorEmail: actorFromContext(ctx),
	}

	if previous != nil {
		if data, err := json.Marshal(flagSnapshot(previous)); err == nil {
			entry.PreviousValue = data
		}
	}
	if current != nil {
		if data, err := json.Marshal(flagSnapshot(current)); err == nil {
			entry.NewValue = data
		}
	}

	if err := s.auditStore.RecordAudit(ctx, entry); err != nil {
		slog.Error("failed to record audit entry", "error", err, "flag_id", flagID, "action", action)
	}
}

func flagSnapshot(f *store.Flag) map[string]any {
	return map[string]any{
		"name":               f.Name,
		"description":        f.Description,
		"type":               f.Type,
		"default_value":      f.DefaultValue,
		"enabled":            f.Enabled,
		"rollout_percentage": f.RolloutPercentage,
		"targeting_rule_id":  f.TargetingRuleID,
		"variant_count":      len(f.Variants),
	}
}

func validateFlag(pb *flagsv1.Flag) error {
	if strings.TrimSpace(pb.GetName()) == "" {
		return fmt.Errorf("name is required")
	}

	if pb.GetType() == flagsv1.FlagType_FLAG_TYPE_UNSPECIFIED {
		return fmt.Errorf("type must be specified")
	}

	if pb.GetRolloutPercentage() < 0.0 || pb.GetRolloutPercentage() > 1.0 {
		return fmt.Errorf("rollout_percentage must be between 0.0 and 1.0")
	}

	if pb.GetType() == flagsv1.FlagType_FLAG_TYPE_BOOLEAN {
		dv := pb.GetDefaultValue()
		if dv != "true" && dv != "false" {
			return fmt.Errorf("boolean flag default_value must be \"true\" or \"false\"")
		}
	}

	if len(pb.GetVariants()) > 0 {
		var sum float64
		for _, v := range pb.GetVariants() {
			if v.GetTrafficFraction() < 0.0 || v.GetTrafficFraction() > 1.0 {
				return fmt.Errorf("variant traffic_fraction must be between 0.0 and 1.0")
			}
			sum += v.GetTrafficFraction()
		}
		if math.Abs(sum-1.0) > 0.001 {
			return fmt.Errorf("variant traffic_fractions must sum to 1.0 (got %f)", sum)
		}
	}

	return nil
}
