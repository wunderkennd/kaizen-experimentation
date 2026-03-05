package handlers

import (
	"github.com/org/experimentation-platform/services/flags/internal/store"
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"
)

func flagToProto(f *store.Flag) *flagsv1.Flag {
	pb := &flagsv1.Flag{
		FlagId:            f.FlagID,
		Name:              f.Name,
		Description:       f.Description,
		Type:              domainTypeToProto(f.Type),
		DefaultValue:      f.DefaultValue,
		Enabled:           f.Enabled,
		RolloutPercentage: f.RolloutPercentage,
		TargetingRuleId:   f.TargetingRuleID,
	}
	for _, v := range f.Variants {
		pb.Variants = append(pb.Variants, &flagsv1.FlagVariant{
			VariantId:       v.VariantID,
			Value:           v.Value,
			TrafficFraction: v.TrafficFraction,
		})
	}
	return pb
}

func protoToFlag(pb *flagsv1.Flag) *store.Flag {
	f := &store.Flag{
		FlagID:            pb.GetFlagId(),
		Name:              pb.GetName(),
		Description:       pb.GetDescription(),
		Type:              protoTypeToDomain(pb.GetType()),
		DefaultValue:      pb.GetDefaultValue(),
		Enabled:           pb.GetEnabled(),
		RolloutPercentage: pb.GetRolloutPercentage(),
		TargetingRuleID:   pb.GetTargetingRuleId(),
	}
	for _, v := range pb.GetVariants() {
		f.Variants = append(f.Variants, store.FlagVariant{
			Value:           v.GetValue(),
			TrafficFraction: v.GetTrafficFraction(),
		})
	}
	return f
}

func domainTypeToProto(t string) flagsv1.FlagType {
	switch t {
	case "BOOLEAN":
		return flagsv1.FlagType_FLAG_TYPE_BOOLEAN
	case "STRING":
		return flagsv1.FlagType_FLAG_TYPE_STRING
	case "NUMERIC":
		return flagsv1.FlagType_FLAG_TYPE_NUMERIC
	case "JSON":
		return flagsv1.FlagType_FLAG_TYPE_JSON
	default:
		return flagsv1.FlagType_FLAG_TYPE_UNSPECIFIED
	}
}

func protoTypeToDomain(t flagsv1.FlagType) string {
	switch t {
	case flagsv1.FlagType_FLAG_TYPE_BOOLEAN:
		return "BOOLEAN"
	case flagsv1.FlagType_FLAG_TYPE_STRING:
		return "STRING"
	case flagsv1.FlagType_FLAG_TYPE_NUMERIC:
		return "NUMERIC"
	case flagsv1.FlagType_FLAG_TYPE_JSON:
		return "JSON"
	default:
		return "BOOLEAN"
	}
}
