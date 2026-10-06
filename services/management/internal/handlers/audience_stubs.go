package handlers

import (
	"context"
	"errors"

	"connectrpc.com/connect"

	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
	audiencev1 "github.com/org/experimentation/gen/go/kaizen/audience/v1"
)

// The AudienceRule RPCs come with the management contract vendored from
// kaizen-rosetta (#822). Neither M5 variant stores or evaluates audience rules
// yet, so these stubs only keep *ExperimentService satisfying the generated
// ExperimentManagementServiceHandler interface; callers get Unimplemented.

var errAudienceRulesUnimplemented = errors.New("audience rules are not implemented in M5 yet (#822)")

// CreateAudienceRule is unimplemented. See #822.
func (s *ExperimentService) CreateAudienceRule(
	ctx context.Context,
	req *connect.Request[mgmtv1.CreateAudienceRuleRequest],
) (*connect.Response[audiencev1.AudienceRule], error) {
	return nil, connect.NewError(connect.CodeUnimplemented, errAudienceRulesUnimplemented)
}

// GetAudienceRule is unimplemented. See #822.
func (s *ExperimentService) GetAudienceRule(
	ctx context.Context,
	req *connect.Request[mgmtv1.GetAudienceRuleRequest],
) (*connect.Response[audiencev1.AudienceRule], error) {
	return nil, connect.NewError(connect.CodeUnimplemented, errAudienceRulesUnimplemented)
}

// ListAudienceRules is unimplemented. See #822.
func (s *ExperimentService) ListAudienceRules(
	ctx context.Context,
	req *connect.Request[mgmtv1.ListAudienceRulesRequest],
) (*connect.Response[mgmtv1.ListAudienceRulesResponse], error) {
	return nil, connect.NewError(connect.CodeUnimplemented, errAudienceRulesUnimplemented)
}
