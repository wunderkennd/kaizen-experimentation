package compute

import (
	"encoding/json"
	"testing"
)

// TestMigrationContainerDef verifies that the migration container definition
// produces valid JSON with the expected structure: entrypoint override,
// command override, no port mappings, and individual secret field extraction.
func TestMigrationContainerDef(t *testing.T) {
	env := "dev"
	dbSecretArn := "arn:aws:secretsmanager:us-east-1:123456789012:secret:kaizen/dev/database-AbCdEf"

	def := containerDef{
		Name:       "db-migration",
		Image:      "123456789012.dkr.ecr.us-east-1.amazonaws.com/kaizen-management:latest",
		Essential:  true,
		EntryPoint: []string{"sh", "-c"},
		Command:    []string{`set -e; migrate -path /app/sql/migrations -database "postgres://${DB_USER}:${DB_PASS}@${DB_HOST}/${DB_NAME}?sslmode=require" up`},
		PortMappings: []portMap{},
		LogConfiguration: logCfg{
			LogDriver: "awslogs",
			Options: map[string]string{
				"awslogs-group":         "/ecs/kaizen-dev/migration",
				"awslogs-region":        "us-east-1",
				"awslogs-stream-prefix": "db-migration",
			},
		},
		Environment: []envKV{
			{Name: "ENVIRONMENT", Value: env},
		},
		Secrets: []secretRef{
			{Name: "DB_HOST", ValueFrom: dbSecretArn + ":host::"},
			{Name: "DB_USER", ValueFrom: dbSecretArn + ":username::"},
			{Name: "DB_PASS", ValueFrom: dbSecretArn + ":password::"},
			{Name: "DB_NAME", ValueFrom: dbSecretArn + ":dbname::"},
		},
	}

	b, err := json.Marshal([]containerDef{def})
	if err != nil {
		t.Fatalf("marshal migration container def: %v", err)
	}

	// Unmarshal to verify round-trip integrity.
	var defs []map[string]interface{}
	if err := json.Unmarshal(b, &defs); err != nil {
		t.Fatalf("unmarshal migration container def: %v", err)
	}
	if len(defs) != 1 {
		t.Fatalf("expected 1 container def, got %d", len(defs))
	}

	d := defs[0]

	// Verify entrypoint override is present.
	ep, ok := d["entryPoint"].([]interface{})
	if !ok {
		t.Fatal("entryPoint missing or wrong type")
	}
	if len(ep) != 2 || ep[0] != "sh" || ep[1] != "-c" {
		t.Errorf("entryPoint: got %v, want [sh, -c]", ep)
	}

	// Verify command override is present.
	cmd, ok := d["command"].([]interface{})
	if !ok {
		t.Fatal("command missing or wrong type")
	}
	if len(cmd) != 1 {
		t.Fatalf("command: expected 1 element, got %d", len(cmd))
	}
	cmdStr := cmd[0].(string)
	if cmdStr == "" {
		t.Error("command string is empty")
	}

	// Verify no port mappings. With omitempty, an empty slice may be
	// omitted entirely from JSON — both cases are valid.
	if ports, ok := d["portMappings"]; ok {
		portsSlice, ok := ports.([]interface{})
		if !ok {
			t.Fatal("portMappings present but wrong type")
		}
		if len(portsSlice) != 0 {
			t.Errorf("expected 0 port mappings, got %d", len(portsSlice))
		}
	}

	// Verify secret references use JSON key extraction syntax.
	secrets, ok := d["secrets"].([]interface{})
	if !ok {
		t.Fatal("secrets missing or wrong type")
	}
	if len(secrets) != 4 {
		t.Fatalf("expected 4 secrets, got %d", len(secrets))
	}

	expectedKeys := map[string]string{
		"DB_HOST": ":host::",
		"DB_USER": ":username::",
		"DB_PASS": ":password::",
		"DB_NAME": ":dbname::",
	}

	for _, s := range secrets {
		sm := s.(map[string]interface{})
		name := sm["name"].(string)
		valueFrom := sm["valueFrom"].(string)
		suffix, exists := expectedKeys[name]
		if !exists {
			t.Errorf("unexpected secret name: %q", name)
			continue
		}
		if len(valueFrom) < len(suffix) || valueFrom[len(valueFrom)-len(suffix):] != suffix {
			t.Errorf("secret %q valueFrom %q does not end with %q", name, valueFrom, suffix)
		}
	}
}

// TestServiceContainerDefBackwardCompat verifies that adding EntryPoint and
// Command fields with omitempty does not change the JSON output for existing
// service container definitions (which don't set these fields).
func TestServiceContainerDefBackwardCompat(t *testing.T) {
	def := containerDef{
		Name:      "m5-management",
		Image:     "example.dkr.ecr.us-east-1.amazonaws.com/kaizen-management:latest",
		Essential: true,
		// EntryPoint and Command deliberately NOT set.
		PortMappings: []portMap{
			{ContainerPort: 50055, Protocol: "tcp"},
		},
		LogConfiguration: logCfg{
			LogDriver: "awslogs",
			Options: map[string]string{
				"awslogs-group":         "/ecs/kaizen-dev",
				"awslogs-region":        "us-east-1",
				"awslogs-stream-prefix": "m5-management",
			},
		},
		Environment: []envKV{{Name: "ENVIRONMENT", Value: "dev"}},
		Secrets:     []secretRef{{Name: "DATABASE_SECRET", ValueFrom: "arn:aws:secretsmanager:..."}},
	}

	b, err := json.Marshal(def)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	// Verify entryPoint and command are NOT present in the JSON.
	var raw map[string]interface{}
	if err := json.Unmarshal(b, &raw); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}

	if _, ok := raw["entryPoint"]; ok {
		t.Error("entryPoint should be omitted when not set (omitempty)")
	}
	if _, ok := raw["command"]; ok {
		t.Error("command should be omitted when not set (omitempty)")
	}
}
