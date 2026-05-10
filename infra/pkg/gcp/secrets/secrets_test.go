package secrets

import (
	"encoding/json"
	"strings"
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/config"
)

// ---------------------------------------------------------------------------
// Pure-function tests — no Pulumi context required
// ---------------------------------------------------------------------------

func TestSecretID(t *testing.T) {
	tests := []struct {
		env       config.Environment
		component string
		want      string
	}{
		{config.EnvDev, "database", "kaizen-dev-database"},
		{config.EnvStaging, "kafka", "kaizen-staging-kafka"},
		{config.EnvProd, "redis", "kaizen-prod-redis"},
		{config.EnvDev, "auth", "kaizen-dev-auth"},
	}
	for _, tt := range tests {
		t.Run(string(tt.env)+"/"+tt.component, func(t *testing.T) {
			cfg := &config.Config{Env: tt.env}
			got := SecretID(cfg, tt.component)
			if got != tt.want {
				t.Errorf("SecretID(%s, %s) = %q, want %q", tt.env, tt.component, got, tt.want)
			}
		})
	}
}

// SecretID must satisfy GCP's [A-Za-z0-9_-]+ rule. Slashes from cfg.SecretPath()
// would be invalid — this guards against future regressions if someone swaps
// the helper.
func TestSecretIDIsGCPCompatible(t *testing.T) {
	cfg := &config.Config{Env: config.EnvDev}
	for _, component := range []string{"database", "kafka", "redis", "auth"} {
		id := SecretID(cfg, component)
		for _, r := range id {
			ok := (r >= 'a' && r <= 'z') ||
				(r >= 'A' && r <= 'Z') ||
				(r >= '0' && r <= '9') ||
				r == '-' || r == '_'
			if !ok {
				t.Errorf("SecretID(%q) = %q contains invalid GCP SecretId char %q", component, id, r)
			}
		}
		if len(id) > 255 {
			t.Errorf("SecretID(%q) = %q exceeds 255 char GCP limit", component, id)
		}
	}
}

func TestDatabaseSecretJSONShape(t *testing.T) {
	v := DatabaseSecret{
		Engine:   "postgres",
		Host:     "10.0.0.1",
		Port:     5432,
		Username: "u",
		Password: "p",
		Dbname:   "d",
	}
	b, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	// Wire shape must match pkg/aws/secrets.DatabaseSecret. Service code
	// reads identical JSON on both clouds.
	wantFields := []string{
		`"engine":"postgres"`,
		`"host":"10.0.0.1"`,
		`"port":5432`,
		`"username":"u"`,
		`"password":"p"`,
		`"dbname":"d"`,
	}
	got := string(b)
	for _, f := range wantFields {
		if !strings.Contains(got, f) {
			t.Errorf("DatabaseSecret JSON missing field %s; got %s", f, got)
		}
	}
}

func TestKafkaSecretJSONShape(t *testing.T) {
	v := KafkaSecret{
		SaslUsername:     "u",
		SaslPassword:     "p",
		SaslMechanism:    "SCRAM-SHA-512",
		BootstrapBrokers: "broker:9092",
	}
	b, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	got := string(b)
	for _, f := range []string{
		`"sasl_username":"u"`,
		`"sasl_password":"p"`,
		`"sasl_mechanism":"SCRAM-SHA-512"`,
		`"bootstrap_brokers":"broker:9092"`,
	} {
		if !strings.Contains(got, f) {
			t.Errorf("KafkaSecret JSON missing field %s; got %s", f, got)
		}
	}
}

func TestRedisSecretJSONShape(t *testing.T) {
	v := RedisSecret{AuthToken: "t", Endpoint: "host", Port: 6379}
	b, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	got := string(b)
	for _, f := range []string{
		`"auth_token":"t"`,
		`"endpoint":"host"`,
		`"port":6379`,
	} {
		if !strings.Contains(got, f) {
			t.Errorf("RedisSecret JSON missing field %s; got %s", f, got)
		}
	}
}

func TestMarshalJSONWrapsError(t *testing.T) {
	// json.Marshal cannot encode a channel; verify the error is wrapped.
	_, err := marshalJSON(make(chan int))
	if err == nil {
		t.Fatal("marshalJSON(chan) returned nil error; want non-nil")
	}
	if !strings.Contains(err.Error(), "marshal secret payload") {
		t.Errorf("error %q does not contain wrapping prefix", err.Error())
	}
}

func TestAuthSecretJSONShape(t *testing.T) {
	v := AuthSecret{
		ClientID:     "id",
		ClientSecret: "secret",
		TokenURL:     "https://t",
		Issuer:       "https://i",
	}
	b, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	got := string(b)
	for _, f := range []string{
		`"client_id":"id"`,
		`"client_secret":"secret"`,
		`"token_url":"https://t"`,
		`"issuer":"https://i"`,
	} {
		if !strings.Contains(got, f) {
			t.Errorf("AuthSecret JSON missing field %s; got %s", f, got)
		}
	}
}

// ---------------------------------------------------------------------------
// Pulumi mock tests — verify resource creation, naming, replication policy
// ---------------------------------------------------------------------------

type trackedResource struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

type secretsMocks struct {
	mu        sync.Mutex
	resources []trackedResource
}

func (m *secretsMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, trackedResource{
		TypeToken: args.TypeToken,
		Name:      args.Name,
		Inputs:    args.Inputs,
	})
	m.mu.Unlock()

	id := args.Name + "_id"
	outputs := resource.PropertyMap{}
	for k, v := range args.Inputs {
		outputs[k] = v
	}

	// Type-specific output enrichment so downstream `secret.Name` references
	// resolve to a realistic-looking GCP path.
	if args.TypeToken == "gcp:secretmanager/secret:Secret" {
		secretID := args.Name
		if v, ok := args.Inputs["secretId"]; ok && v.HasValue() {
			secretID = v.StringValue()
		}
		outputs["name"] = resource.NewStringProperty("projects/test-project/secrets/" + secretID)
		outputs["project"] = resource.NewStringProperty("test-project")
	}
	return id, outputs, nil
}

func (m *secretsMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *secretsMocks) findByType(token string) []trackedResource {
	m.mu.Lock()
	defer m.mu.Unlock()
	out := make([]trackedResource, 0)
	for _, r := range m.resources {
		if r.TypeToken == token {
			out = append(out, r)
		}
	}
	return out
}

func TestNewSecretsCreatesAllFourSecrets(t *testing.T) {
	mocks := &secretsMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg := &config.Config{
			Project: "kaizen",
			Env:     config.EnvDev,
		}
		_, err := NewSecrets(ctx, cfg, &SecretsInputs{
			CloudSqlEndpoint:      pulumi.String("10.0.0.1:5432").ToStringOutput(),
			KafkaBootstrapBrokers: pulumi.String("redpanda:9092").ToStringOutput(),
			RedisEndpoint:         pulumi.String("10.0.0.2").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}

	secretResources := mocks.findByType("gcp:secretmanager/secret:Secret")
	if len(secretResources) != 4 {
		t.Fatalf("expected 4 secrets, got %d", len(secretResources))
	}

	found := make(map[string]bool)
	for _, s := range secretResources {
		v, ok := s.Inputs["secretId"]
		if !ok || !v.HasValue() {
			t.Errorf("secret %q missing secretId input", s.Name)
			continue
		}
		found[v.StringValue()] = true
	}
	for _, want := range []string{
		"kaizen-dev-database",
		"kaizen-dev-kafka",
		"kaizen-dev-redis",
		"kaizen-dev-auth",
	} {
		if !found[want] {
			t.Errorf("missing secret with secretId=%q", want)
		}
	}

	// All 4 must also have a SecretVersion attached.
	versions := mocks.findByType("gcp:secretmanager/secretVersion:SecretVersion")
	if len(versions) != 4 {
		t.Fatalf("expected 4 secret versions, got %d", len(versions))
	}
}

func TestNewSecretsUsesAutomaticReplication(t *testing.T) {
	mocks := &secretsMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg := &config.Config{Project: "kaizen", Env: config.EnvProd}
		_, err := NewSecrets(ctx, cfg, &SecretsInputs{
			CloudSqlEndpoint:      pulumi.String("h:5432").ToStringOutput(),
			KafkaBootstrapBrokers: pulumi.String("b:9092").ToStringOutput(),
			RedisEndpoint:         pulumi.String("r").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "prod", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}

	for _, s := range mocks.findByType("gcp:secretmanager/secret:Secret") {
		repl, ok := s.Inputs["replication"]
		if !ok || !repl.HasValue() {
			t.Errorf("secret %q missing replication policy", s.Name)
			continue
		}
		// Replication is an object; its "auto" key must be present (even if empty
		// — that's how Pulumi serializes the empty-message Auto config).
		if !repl.IsObject() {
			t.Errorf("secret %q replication not an object", s.Name)
			continue
		}
		obj := repl.ObjectValue()
		if _, hasAuto := obj["auto"]; !hasAuto {
			t.Errorf("secret %q replication missing 'auto' key (got keys: %v)", s.Name, keys(obj))
		}
	}
}

func TestNewSecretsAppliesEnvironmentLabels(t *testing.T) {
	mocks := &secretsMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg := &config.Config{Project: "kaizen", Env: config.EnvStaging}
		_, err := NewSecrets(ctx, cfg, &SecretsInputs{
			CloudSqlEndpoint:      pulumi.String("h").ToStringOutput(),
			KafkaBootstrapBrokers: pulumi.String("b").ToStringOutput(),
			RedisEndpoint:         pulumi.String("r").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "staging", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}

	for _, s := range mocks.findByType("gcp:secretmanager/secret:Secret") {
		labels, ok := s.Inputs["labels"]
		if !ok || !labels.HasValue() {
			t.Errorf("secret %q missing labels", s.Name)
			continue
		}
		obj := labels.ObjectValue()
		if v, ok := obj["environment"]; !ok || v.StringValue() != "staging" {
			t.Errorf("secret %q environment label = %v, want staging", s.Name, v)
		}
		if v, ok := obj["managed_by"]; !ok || v.StringValue() != "pulumi" {
			t.Errorf("secret %q managed_by label = %v, want pulumi", s.Name, v)
		}
		if _, ok := obj["component"]; !ok {
			t.Errorf("secret %q missing component label", s.Name)
		}
	}
}

func TestNewSecretsRefsHaveVersionsLatest(t *testing.T) {
	mocks := &secretsMocks{}
	var (
		dbRef, kafkaRef, redisRef, authRef string
	)
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg := &config.Config{Project: "kaizen", Env: config.EnvDev}
		out, err := NewSecrets(ctx, cfg, &SecretsInputs{
			CloudSqlEndpoint:      pulumi.String("h").ToStringOutput(),
			KafkaBootstrapBrokers: pulumi.String("b").ToStringOutput(),
			RedisEndpoint:         pulumi.String("r").ToStringOutput(),
		})
		if err != nil {
			return err
		}

		// Drain refs into local strings using ApplyT; pulumi.RunErr blocks until
		// all outputs resolve, so the values are populated by the time RunErr
		// returns.
		var wg sync.WaitGroup
		wg.Add(4)
		out.DatabaseSecretRef.ApplyT(func(s string) string { dbRef = s; wg.Done(); return s })
		out.KafkaSecretRef.ApplyT(func(s string) string { kafkaRef = s; wg.Done(); return s })
		out.RedisSecretRef.ApplyT(func(s string) string { redisRef = s; wg.Done(); return s })
		out.AuthSecretRef.ApplyT(func(s string) string { authRef = s; wg.Done(); return s })
		wg.Wait()
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}

	for name, ref := range map[string]string{
		"database": dbRef,
		"kafka":    kafkaRef,
		"redis":    redisRef,
		"auth":     authRef,
	} {
		if !strings.HasSuffix(ref, "/versions/latest") {
			t.Errorf("%s ref %q does not end in /versions/latest", name, ref)
		}
		if !strings.HasPrefix(ref, "projects/") {
			t.Errorf("%s ref %q does not start with projects/", name, ref)
		}
		if !strings.Contains(ref, "/secrets/kaizen-dev-"+name) {
			t.Errorf("%s ref %q missing /secrets/kaizen-dev-%s segment", name, ref, name)
		}
	}
}

func TestNewSecretsRejectsNilInputs(t *testing.T) {
	mocks := &secretsMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg := &config.Config{Project: "kaizen", Env: config.EnvDev}
		_, err := NewSecrets(ctx, cfg, nil)
		if err == nil {
			t.Errorf("NewSecrets(nil inputs) returned nil error; want non-nil")
		}
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}
}

// keys returns the keys of a PropertyMap as a sorted-ish slice (for error
// messages — order doesn't matter for assertions).
func keys(m resource.PropertyMap) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, string(k))
	}
	return out
}
