package store

import (
	"encoding/json"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
)

// Internal JSON structures matching the targeting_rules.rule_definition JSONB schema.
type ruleDefinition struct {
	Groups []ruleGroup `json:"groups"`
}

type ruleGroup struct {
	Predicates []rulePredicate `json:"predicates"`
}

type rulePredicate struct {
	AttributeKey string   `json:"attribute_key"`
	Operator     string   `json:"operator"`
	Values       []string `json:"values"`
}

// Operator maps: proto enum ↔ JSONB string.
var operatorToString = map[commonv1.TargetingOperator]string{
	commonv1.TargetingOperator_TARGETING_OPERATOR_EQUALS:       "EQUALS",
	commonv1.TargetingOperator_TARGETING_OPERATOR_NOT_EQUALS:   "NOT_EQUALS",
	commonv1.TargetingOperator_TARGETING_OPERATOR_IN:           "IN",
	commonv1.TargetingOperator_TARGETING_OPERATOR_NOT_IN:       "NOT_IN",
	commonv1.TargetingOperator_TARGETING_OPERATOR_GREATER_THAN: "GREATER_THAN",
	commonv1.TargetingOperator_TARGETING_OPERATOR_LESS_THAN:    "LESS_THAN",
	commonv1.TargetingOperator_TARGETING_OPERATOR_CONTAINS:     "CONTAINS",
	commonv1.TargetingOperator_TARGETING_OPERATOR_REGEX:        "REGEX",
}

var stringToOperator = map[string]commonv1.TargetingOperator{
	"EQUALS":       commonv1.TargetingOperator_TARGETING_OPERATOR_EQUALS,
	"NOT_EQUALS":   commonv1.TargetingOperator_TARGETING_OPERATOR_NOT_EQUALS,
	"IN":           commonv1.TargetingOperator_TARGETING_OPERATOR_IN,
	"NOT_IN":       commonv1.TargetingOperator_TARGETING_OPERATOR_NOT_IN,
	"GREATER_THAN": commonv1.TargetingOperator_TARGETING_OPERATOR_GREATER_THAN,
	"LESS_THAN":    commonv1.TargetingOperator_TARGETING_OPERATOR_LESS_THAN,
	"CONTAINS":     commonv1.TargetingOperator_TARGETING_OPERATOR_CONTAINS,
	"REGEX":        commonv1.TargetingOperator_TARGETING_OPERATOR_REGEX,
}

// TargetingRuleToRow converts a proto TargetingRule to a DB row.
func TargetingRuleToRow(rule *commonv1.TargetingRule) (TargetingRuleRow, error) {
	def := ruleDefinition{}
	for _, g := range rule.GetGroups() {
		group := ruleGroup{}
		for _, p := range g.GetPredicates() {
			group.Predicates = append(group.Predicates, rulePredicate{
				AttributeKey: p.GetAttributeKey(),
				Operator:     operatorToString[p.GetOperator()],
				Values:       p.GetValues(),
			})
		}
		def.Groups = append(def.Groups, group)
	}

	defJSON, err := json.Marshal(def)
	if err != nil {
		return TargetingRuleRow{}, err
	}

	return TargetingRuleRow{
		RuleID:         rule.GetRuleId(),
		Name:           rule.GetName(),
		RuleDefinition: defJSON,
	}, nil
}

// RowToTargetingRule converts a DB row to a proto TargetingRule.
func RowToTargetingRule(row TargetingRuleRow) *commonv1.TargetingRule {
	rule := &commonv1.TargetingRule{
		RuleId: row.RuleID,
		Name:   row.Name,
	}

	var def ruleDefinition
	if err := json.Unmarshal(row.RuleDefinition, &def); err != nil {
		return rule
	}

	for _, g := range def.Groups {
		group := &commonv1.TargetingGroup{}
		for _, p := range g.Predicates {
			group.Predicates = append(group.Predicates, &commonv1.TargetingPredicate{
				AttributeKey: p.AttributeKey,
				Operator:     stringToOperator[p.Operator],
				Values:       p.Values,
			})
		}
		rule.Groups = append(rule.Groups, group)
	}

	return rule
}
