package handlers

import (
	"encoding/json"
	"fmt"
	"log/slog"
	"net/http"
	"strings"
	"time"

	"connectrpc.com/connect"
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
)

// RegisterLinkageRoutes adds internal HTTP endpoints for flag-experiment linkage
// and targeting rule dependency tracking.
func (s *FlagService) RegisterLinkageRoutes(mux *http.ServeMux) {
	mux.HandleFunc("/internal/flags/promoted", s.handleGetPromotedFlags)
	mux.HandleFunc("/internal/flags/by-targeting-rule", s.handleGetFlagsByTargetingRule)
	mux.HandleFunc("/internal/flags/resolve", s.handleResolvePromotedExperiment)
}

// handleGetPromotedFlags lists all flags that have been promoted to experiments.
func (s *FlagService) handleGetPromotedFlags(w http.ResponseWriter, r *http.Request) {
	flags, err := s.store.GetPromotedFlags(r.Context())
	if err != nil {
		http.Error(w, fmt.Sprintf("get promoted flags: %v", err), http.StatusInternalServerError)
		return
	}

	type promotedFlagResponse struct {
		FlagID               string  `json:"flag_id"`
		Name                 string  `json:"name"`
		Enabled              bool    `json:"enabled"`
		RolloutPercentage    float64 `json:"rollout_percentage"`
		PromotedExperimentID string  `json:"promoted_experiment_id"`
		PromotedAt           string  `json:"promoted_at"`
	}

	var resp []promotedFlagResponse
	for _, f := range flags {
		resp = append(resp, promotedFlagResponse{
			FlagID:               f.FlagID,
			Name:                 f.Name,
			Enabled:              f.Enabled,
			RolloutPercentage:    f.RolloutPercentage,
			PromotedExperimentID: f.PromotedExperimentID,
			PromotedAt:           f.PromotedAt.Format("2006-01-02T15:04:05Z"),
		})
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(resp)
}

// handleGetFlagsByTargetingRule returns all flags that reference a given targeting rule.
func (s *FlagService) handleGetFlagsByTargetingRule(w http.ResponseWriter, r *http.Request) {
	ruleID := r.URL.Query().Get("rule_id")
	if ruleID == "" {
		http.Error(w, "rule_id query parameter is required", http.StatusBadRequest)
		return
	}

	flags, err := s.store.GetFlagsByTargetingRule(r.Context(), ruleID)
	if err != nil {
		http.Error(w, fmt.Sprintf("get flags by targeting rule: %v", err), http.StatusInternalServerError)
		return
	}

	type flagRef struct {
		FlagID            string  `json:"flag_id"`
		Name              string  `json:"name"`
		Enabled           bool    `json:"enabled"`
		RolloutPercentage float64 `json:"rollout_percentage"`
	}

	var resp []flagRef
	for _, f := range flags {
		resp = append(resp, flagRef{
			FlagID:            f.FlagID,
			Name:              f.Name,
			Enabled:           f.Enabled,
			RolloutPercentage: f.RolloutPercentage,
		})
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(resp)
}

// ResolutionAction defines how a flag should be updated when its promoted experiment concludes.
type ResolutionAction string

const (
	// ResolutionRolloutFull sets the flag to 100% rollout (treatment won).
	ResolutionRolloutFull ResolutionAction = "rollout_full"
	// ResolutionRollback disables the flag (control won / experiment failed).
	ResolutionRollback ResolutionAction = "rollback"
	// ResolutionKeep leaves the flag unchanged (manual follow-up needed).
	ResolutionKeep ResolutionAction = "keep"
)

// handleResolvePromotedExperiment checks the experiment state via M5 and
// updates the flag based on the resolution action.
//
// POST /internal/flags/resolve?flag_id=...&action=rollout_full|rollback|keep
func (s *FlagService) handleResolvePromotedExperiment(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "POST required", http.StatusMethodNotAllowed)
		return
	}

	flagID := r.URL.Query().Get("flag_id")
	if flagID == "" {
		http.Error(w, "flag_id query parameter is required", http.StatusBadRequest)
		return
	}

	action := ResolutionAction(r.URL.Query().Get("action"))
	if action != ResolutionRolloutFull && action != ResolutionRollback && action != ResolutionKeep {
		http.Error(w, "action must be rollout_full, rollback, or keep", http.StatusBadRequest)
		return
	}

	// Fetch the flag.
	f, err := s.store.GetFlag(r.Context(), flagID)
	if err != nil {
		if strings.Contains(err.Error(), "not found") {
			http.Error(w, "flag not found", http.StatusNotFound)
			return
		}
		http.Error(w, fmt.Sprintf("get flag: %v", err), http.StatusInternalServerError)
		return
	}

	if f.PromotedExperimentID == "" {
		http.Error(w, "flag has not been promoted to an experiment", http.StatusBadRequest)
		return
	}

	// Check experiment state via M5 (if management client is available).
	var experimentState commonv1.ExperimentState
	if s.managementClient != nil {
		resp, err := s.managementClient.GetExperiment(r.Context(), connect.NewRequest(&mgmtv1.GetExperimentRequest{
			ExperimentId: f.PromotedExperimentID,
		}))
		if err != nil {
			slog.Error("M5 GetExperiment failed", "error", err, "experiment_id", f.PromotedExperimentID)
			http.Error(w, fmt.Sprintf("check experiment status: %v", err), http.StatusInternalServerError)
			return
		}
		experimentState = resp.Msg.GetState()

		if experimentState != commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED &&
			experimentState != commonv1.ExperimentState_EXPERIMENT_STATE_ARCHIVED {
			http.Error(w, fmt.Sprintf("experiment is not concluded (current state: %s); cannot resolve yet", experimentState.String()), http.StatusConflict)
			return
		}
	}

	// Apply the resolution action.
	previous := *f
	switch action {
	case ResolutionRolloutFull:
		f.RolloutPercentage = 1.0
		f.Enabled = true
	case ResolutionRollback:
		f.RolloutPercentage = 0.0
		f.Enabled = false
	case ResolutionKeep:
		// No change to the flag itself.
	}

	// Mark as resolved to prevent reconciler from re-processing.
	f.ResolvedAt = time.Now()

	if action != ResolutionKeep {
		if _, err := s.store.UpdateFlag(r.Context(), f); err != nil {
			http.Error(w, fmt.Sprintf("update flag: %v", err), http.StatusInternalServerError)
			return
		}
	}

	s.recordAudit(r.Context(), flagID, "resolve_experiment", &previous, f)

	slog.Info("flag experiment resolved",
		"flag_id", flagID,
		"experiment_id", f.PromotedExperimentID,
		"action", string(action),
		"experiment_state", experimentState.String(),
	)

	type resolveResponse struct {
		FlagID               string `json:"flag_id"`
		PromotedExperimentID string `json:"promoted_experiment_id"`
		Action               string `json:"action"`
		ExperimentState      string `json:"experiment_state"`
		FlagEnabled          bool   `json:"flag_enabled"`
		RolloutPercentage    float64 `json:"rollout_percentage"`
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(resolveResponse{
		FlagID:               flagID,
		PromotedExperimentID: f.PromotedExperimentID,
		Action:               string(action),
		ExperimentState:      experimentState.String(),
		FlagEnabled:          f.Enabled,
		RolloutPercentage:    f.RolloutPercentage,
	})
}
