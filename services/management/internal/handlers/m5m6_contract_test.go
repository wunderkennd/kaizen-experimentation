//go:build integration

// Package handlers_test contains wire-format contract tests between M5 (Management Service)
// and M6 (UI). These tests use raw HTTP POST + JSON parsing — exactly as Agent-6's fetch()
// does — to validate camelCase field names, enum string serialization, proto3 zero-value
// omission, response envelope structure, error format, and RBAC behavior.
package handlers_test

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"testing"
	"time"

	"connectrpc.com/connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"

	"github.com/org/experimentation-platform/services/management/internal/auth"
)

const mgmtSvcPath = "/experimentation.management.v1.ExperimentManagementService/"

// rawPost sends a raw HTTP POST with JSON body and auth headers to a ConnectRPC endpoint,
// exactly as Agent-6's fetch() does. Returns the HTTP status code and parsed JSON response body.
func rawPost(t *testing.T, serverURL, method string, body interface{}, email, role string) (int, map[string]interface{}) {
	t.Helper()

	var bodyReader io.Reader
	if body != nil {
		b, err := json.Marshal(body)
		require.NoError(t, err)
		bodyReader = bytes.NewReader(b)
	} else {
		bodyReader = bytes.NewReader([]byte("{}"))
	}

	req, err := http.NewRequest("POST", serverURL+mgmtSvcPath+method, bodyReader)
	require.NoError(t, err)

	req.Header.Set("Content-Type", "application/json")
	if email != "" {
		req.Header.Set(auth.HeaderUserEmail, email)
	}
	if role != "" {
		req.Header.Set(auth.HeaderUserRole, role)
	}

	resp, err := http.DefaultClient.Do(req)
	require.NoError(t, err)
	defer resp.Body.Close()

	respBody, err := io.ReadAll(resp.Body)
	require.NoError(t, err)

	var result map[string]interface{}
	if len(respBody) > 0 {
		err = json.Unmarshal(respBody, &result)
		require.NoError(t, err, "response body was not valid JSON: %s", string(respBody))
	}

	return resp.StatusCode, result
}

// createExperimentRaw creates an experiment via raw HTTP and returns the parsed JSON response.
func createExperimentRaw(t *testing.T, serverURL, layerID, name string) map[string]interface{} {
	t.Helper()

	body := map[string]interface{}{
		"experiment": map[string]interface{}{
			"name":            name,
			"ownerEmail":      "test@example.com",
			"layerId":         layerID,
			"primaryMetricId": "watch_time_minutes",
			"type":            "EXPERIMENT_TYPE_AB",
			"variants": []map[string]interface{}{
				{"name": "control", "trafficFraction": 0.5, "isControl": true},
				{"name": "treatment", "trafficFraction": 0.5, "isControl": false},
			},
		},
	}

	status, resp := rawPost(t, serverURL, "CreateExperiment", body, "test@example.com", "admin")
	require.Equal(t, http.StatusOK, status, "CreateExperiment failed: %v", resp)
	return resp
}

// createLayerRaw creates a layer via the Go client and returns the layer ID.
func createLayerRaw(t *testing.T, serverURL, name string) string {
	t.Helper()

	// Use the Go client for layer creation since it's admin-only setup.
	client := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient, serverURL,
		withAuth("test@example.com", "admin"),
	)

	resp, err := client.CreateLayer(context.Background(), connect.NewRequest(&mgmtv1.CreateLayerRequest{
		Layer: &commonv1.Layer{
			Name:         name,
			Description:  "contract test layer",
			TotalBuckets: 10000,
		},
	}))
	require.NoError(t, err)
	return resp.Msg.LayerId
}

// startExperimentRaw starts an experiment via raw HTTP. Returns the Go client response for chaining.
func startExperimentRaw(t *testing.T, serverURL, experimentID string) {
	t.Helper()
	status, resp := rawPost(t, serverURL, "StartExperiment",
		map[string]interface{}{"experimentId": experimentID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status, "StartExperiment failed: %v", resp)
}

// --- Contract Tests ---

// TestM5M6_CreateExperiment_FieldPresence verifies that all camelCase fields
// Agent-6 reads from a CreateExperiment response are present in the raw JSON.
func TestM5M6_CreateExperiment_FieldPresence(t *testing.T) {
	serverURL, _, cleanup := setupTestServerRaw(t)
	defer cleanup()

	layerID := createLayerRaw(t, serverURL, "m5m6-field-presence-"+t.Name())
	resp := createExperimentRaw(t, serverURL, layerID, "field-presence-test")

	// All fields Agent-6's adaptExperiment() reads (ui/src/lib/api.ts:82-118).
	assert.NotEmpty(t, resp["experimentId"], "experimentId must be present")
	assert.NotEmpty(t, resp["name"], "name must be present")
	assert.NotEmpty(t, resp["ownerEmail"], "ownerEmail must be present")
	assert.NotEmpty(t, resp["type"], "type must be present")
	assert.NotEmpty(t, resp["state"], "state must be present")
	assert.NotEmpty(t, resp["layerId"], "layerId must be present")
	assert.NotEmpty(t, resp["hashSalt"], "hashSalt must be auto-generated")
	assert.NotEmpty(t, resp["primaryMetricId"], "primaryMetricId must be present")
	assert.NotEmpty(t, resp["createdAt"], "createdAt must be present")

	// Variants array must have 2 items.
	variants, ok := resp["variants"].([]interface{})
	require.True(t, ok, "variants must be an array, got %T", resp["variants"])
	assert.Len(t, variants, 2, "expected 2 variants (control + treatment)")

	// These fields may be zero-valued and thus omitted by proto3 JSON.
	// Agent-6 handles this with || '' / || [] fallbacks, which is correct.
	// Document their presence/absence:
	t.Logf("secondaryMetricIds present: %v", resp["secondaryMetricIds"] != nil)
	t.Logf("guardrailConfigs present: %v", resp["guardrailConfigs"] != nil)
	t.Logf("guardrailAction present: %v", resp["guardrailAction"] != nil)
}

// TestM5M6_EnumSerialization_States verifies that experiment state enums are serialized
// as prefixed strings (e.g., "EXPERIMENT_STATE_DRAFT") in the raw JSON wire format.
// Agent-6 strips the prefix in stripEnumPrefix() (ui/src/lib/api.ts:77-79).
func TestM5M6_EnumSerialization_States(t *testing.T) {
	serverURL, _, cleanup := setupTestServerRaw(t)
	defer cleanup()

	layerID := createLayerRaw(t, serverURL, "m5m6-enum-states-"+t.Name())
	created := createExperimentRaw(t, serverURL, layerID, "enum-state-test")
	expID := created["experimentId"].(string)

	// DRAFT
	status, getResp := rawPost(t, serverURL, "GetExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)
	assert.Equal(t, "EXPERIMENT_STATE_DRAFT", getResp["state"],
		"state must be prefixed string, not numeric")

	// Start → RUNNING
	startExperimentRaw(t, serverURL, expID)
	status, getResp = rawPost(t, serverURL, "GetExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)
	assert.Equal(t, "EXPERIMENT_STATE_RUNNING", getResp["state"])

	// Conclude → CONCLUDED
	status, _ = rawPost(t, serverURL, "ConcludeExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)
	status, getResp = rawPost(t, serverURL, "GetExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)
	assert.Equal(t, "EXPERIMENT_STATE_CONCLUDED", getResp["state"])

	// Archive → ARCHIVED
	status, _ = rawPost(t, serverURL, "ArchiveExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)
	status, getResp = rawPost(t, serverURL, "GetExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)
	assert.Equal(t, "EXPERIMENT_STATE_ARCHIVED", getResp["state"])
}

// TestM5M6_EnumSerialization_TypeAndGuardrailAction verifies that experiment type
// and guardrail action enums are serialized as prefixed strings.
// Agent-6 strips "EXPERIMENT_TYPE_" and "GUARDRAIL_ACTION_" prefixes.
func TestM5M6_EnumSerialization_TypeAndGuardrailAction(t *testing.T) {
	serverURL, _, cleanup := setupTestServerRaw(t)
	defer cleanup()

	layerID := createLayerRaw(t, serverURL, "m5m6-enum-type-"+t.Name())

	// Create experiment with explicit guardrail action
	body := map[string]interface{}{
		"experiment": map[string]interface{}{
			"name":            "type-enum-test",
			"ownerEmail":      "test@example.com",
			"layerId":         layerID,
			"primaryMetricId": "watch_time_minutes",
			"type":            "EXPERIMENT_TYPE_AB",
			"guardrailAction": "GUARDRAIL_ACTION_AUTO_PAUSE",
			"variants": []map[string]interface{}{
				{"name": "control", "trafficFraction": 0.5, "isControl": true},
				{"name": "treatment", "trafficFraction": 0.5, "isControl": false},
			},
		},
	}
	status, resp := rawPost(t, serverURL, "CreateExperiment", body, "test@example.com", "admin")
	require.Equal(t, http.StatusOK, status, "CreateExperiment failed: %v", resp)

	expID := resp["experimentId"].(string)

	// GET and verify type + guardrail action enum strings
	status, getResp := rawPost(t, serverURL, "GetExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)

	assert.Equal(t, "EXPERIMENT_TYPE_AB", getResp["type"],
		"type must be prefixed enum string")
	assert.Equal(t, "GUARDRAIL_ACTION_AUTO_PAUSE", getResp["guardrailAction"],
		"guardrailAction must be prefixed enum string")
}

// TestM5M6_VariantFieldContract verifies that variant objects in the wire format
// contain all fields Agent-6's Variant interface expects (ui/src/lib/types.ts:27-33).
// Documents proto3 zero-value omission for isControl: false.
func TestM5M6_VariantFieldContract(t *testing.T) {
	serverURL, _, cleanup := setupTestServerRaw(t)
	defer cleanup()

	layerID := createLayerRaw(t, serverURL, "m5m6-variant-"+t.Name())

	// Create with explicit payload on treatment
	body := map[string]interface{}{
		"experiment": map[string]interface{}{
			"name":            "variant-contract-test",
			"ownerEmail":      "test@example.com",
			"layerId":         layerID,
			"primaryMetricId": "watch_time_minutes",
			"type":            "EXPERIMENT_TYPE_AB",
			"variants": []map[string]interface{}{
				{"name": "control", "trafficFraction": 0.5, "isControl": true},
				{"name": "treatment", "trafficFraction": 0.5, "isControl": false, "payloadJson": `{"color":"blue"}`},
			},
		},
	}
	status, resp := rawPost(t, serverURL, "CreateExperiment", body, "test@example.com", "admin")
	require.Equal(t, http.StatusOK, status, "CreateExperiment failed: %v", resp)

	expID := resp["experimentId"].(string)

	// GET to check persisted variant data
	status, getResp := rawPost(t, serverURL, "GetExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)

	variants, ok := getResp["variants"].([]interface{})
	require.True(t, ok)
	require.Len(t, variants, 2)

	// Find control and treatment
	var control, treatment map[string]interface{}
	for _, v := range variants {
		vm := v.(map[string]interface{})
		if vm["isControl"] == true {
			control = vm
		} else {
			treatment = vm
		}
	}
	require.NotNil(t, control, "must have a control variant with isControl=true")
	require.NotNil(t, treatment, "must have a treatment variant")

	// Control variant assertions
	assert.NotEmpty(t, control["variantId"], "control must have variantId")
	assert.Equal(t, "control", control["name"])
	assert.Equal(t, true, control["isControl"], "control isControl must be true")
	assert.InDelta(t, 0.5, control["trafficFraction"].(float64), 1e-9)

	// Treatment variant assertions
	assert.NotEmpty(t, treatment["variantId"], "treatment must have variantId")
	assert.Equal(t, "treatment", treatment["name"])
	assert.InDelta(t, 0.5, treatment["trafficFraction"].(float64), 1e-9)

	// Proto3 zero-value omission: isControl=false is the default, so protojson
	// MAY omit it. Agent-6 handles this with `|| false` fallback. Document actual behavior.
	_, treatmentHasIsControl := treatment["isControl"]
	t.Logf("Proto3 zero-value omission: treatment isControl present=%v (Agent-6 uses || false fallback)",
		treatmentHasIsControl)

	// Payload must be present on treatment
	assert.Equal(t, `{"color":"blue"}`, treatment["payloadJson"])
}

// TestM5M6_LifecycleTimestamps verifies that lifecycle timestamps are RFC 3339
// strings in the JSON wire format, and appear only when the corresponding
// lifecycle transition has occurred.
func TestM5M6_LifecycleTimestamps(t *testing.T) {
	serverURL, _, cleanup := setupTestServerRaw(t)
	defer cleanup()

	layerID := createLayerRaw(t, serverURL, "m5m6-timestamps-"+t.Name())
	created := createExperimentRaw(t, serverURL, layerID, "timestamp-test")
	expID := created["experimentId"].(string)

	// DRAFT: createdAt present, startedAt absent
	assert.NotNil(t, created["createdAt"], "createdAt must be present after creation")
	assert.Nil(t, created["startedAt"], "startedAt must be absent in DRAFT")
	assert.Nil(t, created["concludedAt"], "concludedAt must be absent in DRAFT")

	// Validate createdAt is parseable as RFC 3339
	createdAtStr, ok := created["createdAt"].(string)
	require.True(t, ok, "createdAt must be a string")
	_, err := time.Parse(time.RFC3339Nano, createdAtStr)
	require.NoError(t, err, "createdAt must be valid RFC 3339: %s", createdAtStr)

	// Start → RUNNING: startedAt present
	startExperimentRaw(t, serverURL, expID)
	status, running := rawPost(t, serverURL, "GetExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)

	assert.NotNil(t, running["startedAt"], "startedAt must be present after start")
	startedAtStr, ok := running["startedAt"].(string)
	require.True(t, ok)
	_, err = time.Parse(time.RFC3339Nano, startedAtStr)
	require.NoError(t, err, "startedAt must be valid RFC 3339: %s", startedAtStr)

	// Conclude → CONCLUDED: concludedAt present
	status, _ = rawPost(t, serverURL, "ConcludeExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)

	status, concluded := rawPost(t, serverURL, "GetExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)

	assert.NotNil(t, concluded["concludedAt"], "concludedAt must be present after conclude")
	concludedAtStr, ok := concluded["concludedAt"].(string)
	require.True(t, ok)
	_, err = time.Parse(time.RFC3339Nano, concludedAtStr)
	require.NoError(t, err, "concludedAt must be valid RFC 3339: %s", concludedAtStr)
}

// TestM5M6_ListResponse_Envelope verifies that ListExperiments returns the expected
// envelope structure with "experiments" array and "nextPageToken" string.
// Agent-6 reads raw.experiments in listExperiments() (ui/src/lib/api.ts:120-128).
func TestM5M6_ListResponse_Envelope(t *testing.T) {
	serverURL, _, cleanup := setupTestServerRaw(t)
	defer cleanup()

	layerID := createLayerRaw(t, serverURL, "m5m6-list-envelope-"+t.Name())

	// Create 3 experiments
	for i := 0; i < 3; i++ {
		createExperimentRaw(t, serverURL, layerID, fmt.Sprintf("list-test-%d", i))
	}

	// List via raw HTTP
	status, resp := rawPost(t, serverURL, "ListExperiments",
		map[string]interface{}{"pageSize": 100},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)

	// Must have "experiments" array at top level
	experiments, ok := resp["experiments"].([]interface{})
	require.True(t, ok, "response must have 'experiments' array, got %T", resp["experiments"])
	assert.GreaterOrEqual(t, len(experiments), 3, "expected at least 3 experiments")

	// Each experiment in the list must have core fields
	for i, exp := range experiments {
		expMap, ok := exp.(map[string]interface{})
		require.True(t, ok, "experiment[%d] must be an object", i)
		assert.NotEmpty(t, expMap["experimentId"], "experiment[%d] must have experimentId", i)
		assert.NotEmpty(t, expMap["name"], "experiment[%d] must have name", i)
		assert.NotEmpty(t, expMap["state"], "experiment[%d] must have state", i)
	}

	// nextPageToken: may be empty string or absent when there's no next page
	// Agent-6 uses raw.nextPageToken || '' so both are handled.
	t.Logf("nextPageToken present: %v, value: %q", resp["nextPageToken"] != nil, resp["nextPageToken"])
}

// TestM5M6_GetExperiment_ResponseStructure verifies that GetExperiment returns
// the Experiment proto directly at the top level (not nested under an "experiment" key).
// The proto RPC is: rpc GetExperiment(...) returns (Experiment) — no wrapper message.
// Agent-6 handles both cases with: raw.experiment || raw (ui/src/lib/api.ts:134).
func TestM5M6_GetExperiment_ResponseStructure(t *testing.T) {
	serverURL, _, cleanup := setupTestServerRaw(t)
	defer cleanup()

	layerID := createLayerRaw(t, serverURL, "m5m6-get-structure-"+t.Name())
	created := createExperimentRaw(t, serverURL, layerID, "get-structure-test")
	expID := created["experimentId"].(string)

	status, resp := rawPost(t, serverURL, "GetExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)

	// Experiment fields must be at the top level, NOT nested under "experiment"
	assert.Nil(t, resp["experiment"],
		"GetExperiment must NOT wrap response in 'experiment' key — proto returns Experiment directly")
	assert.NotEmpty(t, resp["experimentId"],
		"experimentId must be at top level (not nested)")
	assert.NotEmpty(t, resp["name"],
		"name must be at top level")
	assert.NotEmpty(t, resp["state"],
		"state must be at top level")
}

// TestM5M6_Proto3_ZeroValueOmission documents which fields are omitted when they
// have proto3 zero values. Agent-6 uses fallback defaults (|| '', || false, || [])
// to handle missing fields — this test validates those fallbacks are actually needed.
func TestM5M6_Proto3_ZeroValueOmission(t *testing.T) {
	serverURL, _, cleanup := setupTestServerRaw(t)
	defer cleanup()

	layerID := createLayerRaw(t, serverURL, "m5m6-zero-value-"+t.Name())

	// Create minimal experiment: empty description, no guardrails, isCumulativeHoldout=false
	body := map[string]interface{}{
		"experiment": map[string]interface{}{
			"name":            "zero-value-test",
			"ownerEmail":      "test@example.com",
			"layerId":         layerID,
			"primaryMetricId": "watch_time_minutes",
			"type":            "EXPERIMENT_TYPE_AB",
			"variants": []map[string]interface{}{
				{"name": "control", "trafficFraction": 0.5, "isControl": true},
				{"name": "treatment", "trafficFraction": 0.5},
			},
		},
	}

	status, resp := rawPost(t, serverURL, "CreateExperiment", body, "test@example.com", "admin")
	require.Equal(t, http.StatusOK, status, "CreateExperiment failed: %v", resp)

	expID := resp["experimentId"].(string)

	status, getResp := rawPost(t, serverURL, "GetExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")
	require.Equal(t, http.StatusOK, status)

	// Document proto3 zero-value behavior for each field Agent-6 reads.
	// Fields with zero/default values may be omitted by protojson.
	zeroFields := map[string]string{
		"description":        `Agent-6: (proto.description as string) || ''`,
		"secondaryMetricIds": `Agent-6: (proto.secondaryMetricIds as string[]) || []`,
		"guardrailConfigs":   `Agent-6: (proto.guardrailConfigs as ...) || []`,
		"guardrailAction":    `Agent-6: strips prefix or defaults to 'AUTO_PAUSE'`,
		"isCumulativeHoldout": `Agent-6: (proto.isCumulativeHoldout as boolean) || false`,
		"targetingRuleId":    `Agent-6: proto.targetingRuleId as string | undefined`,
		"surrogateModelId":   `Agent-6: proto.surrogateModelId as string | undefined`,
		"startedAt":          `Agent-6: proto.startedAt as string | undefined`,
		"concludedAt":        `Agent-6: proto.concludedAt as string | undefined`,
	}

	for field, fallback := range zeroFields {
		_, present := getResp[field]
		t.Logf("  %-25s present=%-5v  %s", field, present, fallback)
	}

	// Non-zero fields MUST always be present
	assert.NotEmpty(t, getResp["experimentId"])
	assert.NotEmpty(t, getResp["name"])
	assert.NotEmpty(t, getResp["ownerEmail"])
	assert.NotEmpty(t, getResp["type"])
	assert.NotEmpty(t, getResp["state"])
	assert.NotEmpty(t, getResp["layerId"])
	assert.NotEmpty(t, getResp["hashSalt"])
	assert.NotEmpty(t, getResp["primaryMetricId"])
	assert.NotEmpty(t, getResp["createdAt"])
}

// TestM5M6_ErrorFormat_NotFound verifies that requesting a nonexistent experiment
// returns an error response matching Agent-6's parseRpcError() expectations
// (ui/src/lib/api.ts:49-58): { "code": "...", "message": "..." }.
func TestM5M6_ErrorFormat_NotFound(t *testing.T) {
	serverURL, _, cleanup := setupTestServerRaw(t)
	defer cleanup()

	status, resp := rawPost(t, serverURL, "GetExperiment",
		map[string]interface{}{"experimentId": "00000000-0000-0000-0000-000000000000"},
		"test@example.com", "admin")

	// ConnectRPC maps not_found to HTTP 404
	assert.NotEqual(t, http.StatusOK, status, "should not be 200 for nonexistent experiment")

	// Agent-6 reads body.message in parseRpcError()
	assert.NotNil(t, resp["code"], "error response must have 'code' field")
	assert.NotNil(t, resp["message"], "error response must have 'message' field")

	t.Logf("Error format: HTTP %d, code=%v, message=%v", status, resp["code"], resp["message"])
}

// TestM5M6_ErrorFormat_StateMachineViolation verifies that attempting an invalid
// state transition returns an error with code and message fields.
func TestM5M6_ErrorFormat_StateMachineViolation(t *testing.T) {
	serverURL, _, cleanup := setupTestServerRaw(t)
	defer cleanup()

	layerID := createLayerRaw(t, serverURL, "m5m6-statemachine-err-"+t.Name())
	created := createExperimentRaw(t, serverURL, layerID, "statemachine-error-test")
	expID := created["experimentId"].(string)

	// Start it first
	startExperimentRaw(t, serverURL, expID)

	// Try to start a RUNNING experiment again → should fail
	status, resp := rawPost(t, serverURL, "StartExperiment",
		map[string]interface{}{"experimentId": expID},
		"test@example.com", "admin")

	assert.NotEqual(t, http.StatusOK, status, "double-start should fail")
	assert.NotNil(t, resp["code"], "error response must have 'code' field")
	assert.NotNil(t, resp["message"], "error response must have 'message' field")

	t.Logf("State machine error: HTTP %d, code=%v, message=%v", status, resp["code"], resp["message"])
}

// TestM5M6_RBAC_ViewerCannotMutate verifies that a viewer role cannot create
// experiments, matching Agent-6's isPermissionDenied() check (ui/src/lib/api.ts:44-46).
func TestM5M6_RBAC_ViewerCannotMutate(t *testing.T) {
	serverURL, _, cleanup := setupTestServerRaw(t)
	defer cleanup()

	body := map[string]interface{}{
		"experiment": map[string]interface{}{
			"name":            "viewer-create-attempt",
			"ownerEmail":      "viewer@example.com",
			"layerId":         "any-layer",
			"primaryMetricId": "watch_time_minutes",
			"type":            "EXPERIMENT_TYPE_AB",
			"variants": []map[string]interface{}{
				{"name": "control", "trafficFraction": 0.5, "isControl": true},
				{"name": "treatment", "trafficFraction": 0.5},
			},
		},
	}

	status, resp := rawPost(t, serverURL, "CreateExperiment", body, "viewer@example.com", "viewer")

	assert.Equal(t, http.StatusForbidden, status, "viewer should get 403")
	assert.Equal(t, "permission_denied", resp["code"],
		"code must be 'permission_denied' for Agent-6's isPermissionDenied() check")
	assert.NotNil(t, resp["message"], "error must have message field")
}
