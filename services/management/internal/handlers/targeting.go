package handlers

import (
	"context"
	"log/slog"

	"connectrpc.com/connect"
	"github.com/google/uuid"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"

	"github.com/org/experimentation-platform/services/management/internal/store"
	"github.com/org/experimentation-platform/services/management/internal/validation"
)

// CreateTargetingRule validates the request, inserts the targeting rule,
// and returns the created rule.
func (s *ExperimentService) CreateTargetingRule(
	ctx context.Context,
	req *connect.Request[mgmtv1.CreateTargetingRuleRequest],
) (*connect.Response[commonv1.TargetingRule], error) {
	rule := req.Msg.GetRule()

	if err := validation.ValidateCreateTargetingRule(rule); err != nil {
		return nil, err
	}

	row, err := store.TargetingRuleToRow(rule)
	if err != nil {
		return nil, internalError("convert targeting rule", err)
	}

	if row.RuleID == "" {
		row.RuleID = uuid.NewString()
	}

	created, err := s.targeting.Insert(ctx, row)
	if err != nil {
		return nil, wrapDBError(err, "targeting_rule", row.RuleID)
	}

	slog.Info("targeting rule created", "rule_id", created.RuleID, "name", created.Name)
	return connect.NewResponse(store.RowToTargetingRule(created)), nil
}
