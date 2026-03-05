package handlers

import (
	"encoding/json"
	"fmt"
	"net/http"
	"strconv"
	"time"
)

// RegisterAuditRoutes adds internal HTTP endpoints for flag audit and stale detection.
func (s *FlagService) RegisterAuditRoutes(mux *http.ServeMux) {
	mux.HandleFunc("/internal/flags/audit", s.handleGetFlagAuditLog)
	mux.HandleFunc("/internal/flags/stale", s.handleGetStaleFlags)
}

func (s *FlagService) handleGetFlagAuditLog(w http.ResponseWriter, r *http.Request) {
	if s.auditStore == nil {
		http.Error(w, "audit store not configured", http.StatusServiceUnavailable)
		return
	}

	flagID := r.URL.Query().Get("flag_id")
	if flagID == "" {
		http.Error(w, "flag_id query parameter is required", http.StatusBadRequest)
		return
	}

	limit := 100
	if limitStr := r.URL.Query().Get("limit"); limitStr != "" {
		if parsed, err := strconv.Atoi(limitStr); err == nil && parsed > 0 {
			limit = parsed
		}
	}

	entries, err := s.auditStore.GetFlagAuditLog(r.Context(), flagID, limit)
	if err != nil {
		http.Error(w, fmt.Sprintf("get audit log: %v", err), http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(entries)
}

func (s *FlagService) handleGetStaleFlags(w http.ResponseWriter, r *http.Request) {
	if s.auditStore == nil {
		http.Error(w, "audit store not configured", http.StatusServiceUnavailable)
		return
	}

	thresholdDays := 90
	if daysStr := r.URL.Query().Get("threshold_days"); daysStr != "" {
		if parsed, err := strconv.Atoi(daysStr); err == nil && parsed > 0 {
			thresholdDays = parsed
		}
	}

	threshold := time.Duration(thresholdDays) * 24 * time.Hour
	staleFlags, err := s.auditStore.GetStaleFlags(r.Context(), threshold)
	if err != nil {
		http.Error(w, fmt.Sprintf("get stale flags: %v", err), http.StatusInternalServerError)
		return
	}

	type staleFlagResponse struct {
		FlagID            string  `json:"flag_id"`
		Name              string  `json:"name"`
		Description       string  `json:"description"`
		Type              string  `json:"type"`
		RolloutPercentage float64 `json:"rollout_percentage"`
		DaysSinceUpdate   int     `json:"days_since_update"`
		Suggestion        string  `json:"suggestion"`
	}

	var resp []staleFlagResponse
	for _, sf := range staleFlags {
		daysSinceUpdate := int(sf.StaleDuration.Hours() / 24)
		resp = append(resp, staleFlagResponse{
			FlagID:            sf.FlagID,
			Name:              sf.Name,
			Description:       sf.Description,
			Type:              sf.Type,
			RolloutPercentage: sf.RolloutPercentage,
			DaysSinceUpdate:   daysSinceUpdate,
			Suggestion:        fmt.Sprintf("Flag '%s' has been at 100%% rollout for %d days. Consider removing the flag and making the behavior permanent.", sf.Name, daysSinceUpdate),
		})
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(resp)
}
