package validation

import (
	"fmt"

	"connectrpc.com/connect"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
)

// ValidateCreateTargetingRule validates a targeting rule for creation.
func ValidateCreateTargetingRule(rule *commonv1.TargetingRule) *connect.Error {
	if rule == nil {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("rule is required"))
	}
	if rule.GetName() == "" {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("name is required"))
	}
	if len(rule.GetGroups()) == 0 {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("at least one targeting group is required"))
	}

	for i, group := range rule.GetGroups() {
		if len(group.GetPredicates()) == 0 {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("group[%d] must have at least one predicate", i))
		}
		for j, pred := range group.GetPredicates() {
			if pred.GetAttributeKey() == "" {
				return connect.NewError(connect.CodeInvalidArgument,
					fmt.Errorf("group[%d].predicate[%d]: attribute_key is required", i, j))
			}
			if pred.GetOperator() == commonv1.TargetingOperator_TARGETING_OPERATOR_UNSPECIFIED {
				return connect.NewError(connect.CodeInvalidArgument,
					fmt.Errorf("group[%d].predicate[%d]: operator is required", i, j))
			}
		}
	}

	return nil
}
