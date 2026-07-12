package main

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/aws"
	"github.com/kaizen-experimentation/infra/pkg/aws/compute"
	"github.com/kaizen-experimentation/infra/pkg/aws/loadbalancer"
	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp"
	"github.com/kaizen-experimentation/infra/pkg/gcp/services"
	cloudstreaming "github.com/kaizen-experimentation/infra/pkg/streaming"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// Deploy is the main Pulumi program. It dispatches each of the six
// infrastructure stages on cfg.CloudProvider so that AWS and (future) GCP
// implementations can be slotted in independently. Each stage returns one of
// the shared types.*Outputs structs so subsequent stages can compose without
// knowing which cloud they are running on.
//
// For Phase 0, only the "aws" branch is implemented; "gcp" returns an
// explicit not-yet-implemented error. Stack config that omits cloudProvider
// defaults to "aws" so existing AWS stacks remain byte-for-byte unchanged.
func Deploy(ctx *pulumi.Context) error {
	cfg := kconfig.LoadConfig(ctx)
	ctx.Export("environment", pulumi.String(cfg.Environment))

	// =====================================================================
	// Stage 1: Network Foundation
	// =====================================================================
	var (
		netOut types.NetworkOutputs
		err    error
	)
	switch cfg.CloudProvider {
	case "aws":
		netOut, err = aws.NewNetwork(ctx, cfg)
	case "gcp":
		netOut, err = gcp.NewNetwork(ctx, cfg)
	default:
		return unsupportedCloud(cfg.CloudProvider)
	}
	if err != nil {
		return err
	}

	// =====================================================================
	// Stage 2: Storage + IAM
	// =====================================================================
	var (
		storageOut types.StorageOutputs
		iamOut     types.IAMOutputs
	)
	switch cfg.CloudProvider {
	case "aws":
		storageOut, err = aws.NewStorage(ctx, cfg, netOut)
		if err != nil {
			return err
		}
		iamOut, err = aws.NewIAM(ctx, cfg, storageOut)
	case "gcp":
		storageOut, err = gcp.NewStorage(ctx, cfg, netOut)
		// IAM (Workload Identity) is wired by the compute Phase 1 PR — it
		// owns the runtime service accounts that bind to bucket roles.
	default:
		return unsupportedCloud(cfg.CloudProvider)
	}
	if err != nil {
		return err
	}
	_ = iamOut // exported via ctx.Export inside NewIAM; reserved for future stages

	// =====================================================================
	// Stage 3: Data Stores
	// =====================================================================
	var (
		cacheOut types.CacheOutputs
		dbOut    types.DatabaseOutputs
	)
	switch cfg.CloudProvider {
	case "aws":
		cacheOut, err = aws.NewCache(ctx, cfg, netOut)
		if err != nil {
			return err
		}
		dbOut, err = aws.NewDatabase(ctx, cfg, netOut)
	case "gcp":
		cacheOut, err = gcp.NewCache(ctx, cfg, netOut)
		if err != nil {
			return err
		}
		dbOut, err = gcp.NewDatabase(ctx, cfg, netOut)
	default:
		return unsupportedCloud(cfg.CloudProvider)
	}
	if err != nil {
		return err
	}

	// =====================================================================
	// GCP early return — Phase 1 storage + cache + database + streaming +
	// secrets + cicd + M4b + Cloud Run compute slice + Phase 3 edge
	// =====================================================================
	// Stage 4 (streaming + secrets) is wired: the per-service Cloud Run
	// deploys (#488..#495) need Redpanda bootstrap brokers + Secret Manager
	// refs threaded into compute. Stage 6 edge (#496) fronts the Cloud Run
	// services with the global external HTTPS LB + Cloud DNS + managed cert
	// + Cloud Armor. We run every wired GCP stage and return cleanly so
	// `pulumi preview --stack gcp-dev` succeeds. Observability (#497) is the
	// remaining GCP stage; its PR removes this early return entirely.
	if cfg.CloudProvider == "gcp" {
		// Required-config gate. Validated upfront so the fail-fast behavior
		// (and `TestFullStackDeploy_GCP_RejectsMissingProject`) is preserved
		// regardless of where each downstream stage's own config check lives
		// — without this, the streaming stage below would shadow the
		// gcp.NewCICD check by erroring on the default streamingProvider=msk.
		if cfg.GCPProjectID == "" {
			return fmt.Errorf(
				"cloudProvider=gcp requires kaizen-experimentation:gcpProjectId " +
					"(set via `pulumi config set kaizen-experimentation:gcpProjectId <ID>`)")
		}

		// ── Stage 4a: Streaming ──────────────────────────────────────────
		// GCP tenants use Redpanda Cloud (cloud-agnostic module, gated on
		// streamingProvider). MSK is AWS-only, so reject any other value
		// loudly rather than silently shipping a brokerless stack.
		var gcpStreamOut types.StreamingOutputs
		switch cfg.StreamingProvider {
		case "redpanda":
			gcpStreamOut, err = cloudstreaming.NewRedpanda(ctx, cfg, netOut)
		default:
			// streamingProvider defaults to "msk" when unset (see
			// pkg/config/config.go), so a GCP stack with the key omitted will
			// land here. Spell out the remedy so operators don't have to
			// chase the default through config code.
			return fmt.Errorf(
				"cloudProvider=gcp requires streamingProvider=redpanda (got %q — "+
					"MSK is AWS-only; set explicitly with "+
					"`pulumi config set kaizen-experimentation:streamingProvider redpanda`)",
				cfg.StreamingProvider)
		}
		if err != nil {
			return err
		}

		// ── Stage 4b: Secrets ────────────────────────────────────────────
		gcpSecretsOut, err := gcp.NewSecrets(ctx, cfg, dbOut, gcpStreamOut, cacheOut)
		if err != nil {
			return err
		}

		// ── Stage 4c: CICD ───────────────────────────────────────────────
		cicdOut, err := gcp.NewCICD(ctx, cfg)
		if err != nil {
			return err
		}
		if url, ok := cicdOut.RepositoryURLs["assignment"]; ok {
			ctx.Export("cicdAssignmentRepositoryUrl", url)
		}

		// ── Stage 5: Compute ─────────────────────────────────────────────
		// gcp.NewCompute provisions the stateful M4b slice (#487), the Cloud
		// Run service factory + canary (#486), and the per-service stateless
		// Cloud Run deploys as they land — M2 Orchestration via #490. Sibling
		// services (#488, #489, #491..#495) extend the same call.
		gcpComputeOut, gcpSvcs, err := gcp.NewCompute(ctx, cfg, services.StageOutputs{
			Net: netOut, CICD: cicdOut, DB: dbOut, Cache: cacheOut,
			Stream: gcpStreamOut, Secrets: gcpSecretsOut, Storage: storageOut,
		})
		if err != nil {
			return err
		}
		ctx.Export("streamingBootstrapBrokers", gcpStreamOut.BootstrapBrokers)
		ctx.Export("m4bAsgName", gcpComputeOut.M4bAsgName)
		ctx.Export("m4bInstanceId", gcpComputeOut.M4bInstanceId)
		ctx.Export("m4bEndpointAddress", gcpComputeOut.M4bEndpoint)
		for name, url := range gcpComputeOut.ServiceEndpoints {
			ctx.Export("cloudRunUrl_"+name, url)
		}

		// ── Stage 6: Edge (#496) ─────────────────────────────────────────
		// Global external HTTPS LB with serverless NEGs over the Cloud Run
		// services, Cloud DNS, managed cert, Cloud Armor at WAF v2 parity.
		gcpEdgeOut, err := gcp.NewEdge(ctx, cfg, gcp.EdgeBackends(gcpSvcs))
		if err != nil {
			return err
		}

		ctx.Export("loadBalancerDns", gcpEdgeOut.LoadBalancerDns)
		ctx.Export("dataBucket", storageOut.DataBucketName)
		ctx.Export("cacheEndpoint", cacheOut.Endpoint)
		ctx.Export("databaseEndpoint", dbOut.Endpoint)
		return nil
	}

	// =====================================================================
	// Stage 4: Streaming + Secrets + CICD
	// =====================================================================
	var (
		streamOut  types.StreamingOutputs
		secretsOut types.SecretsOutputs
		cicdOut    types.CICDOutputs
	)
	switch cfg.StreamingProvider {
	case "msk":
		streamOut, err = aws.NewKafkaCluster(ctx, cfg, netOut)
	case "redpanda":
		// Redpanda Cloud bundles cluster, users/ACLs, topics, and a built-in
		// Schema Registry — so the dispatch is self-contained and does NOT
		// flow through aws.NewKafkaTopics or aws.NewSchemaRegistry below.
		streamOut, err = cloudstreaming.NewRedpanda(ctx, cfg, netOut)
	default:
		return fmt.Errorf("unsupported streamingProvider %q (expected \"msk\" or \"redpanda\")", cfg.StreamingProvider)
	}
	if err != nil {
		return err
	}
	// MSK is the only streaming provider that requires the AWS-side helper
	// stages — separate topic provisioning, a standalone Confluent Schema
	// Registry on ECS, and the Schema Registry health gate. Redpanda Cloud
	// ships all three inside NewRedpanda, so those stages are skipped.
	// A single source-of-truth predicate prevents the topic-creation gate
	// and the schema-registry gate from drifting out of sync.
	needsAWSStreamingStages := cfg.StreamingProvider == "msk"
	switch cfg.CloudProvider {
	case "aws":
		secretsOut, err = aws.NewSecrets(ctx, cfg, dbOut, streamOut, cacheOut)
		if err != nil {
			return err
		}
		// MSK requires a Pulumi-managed kafka provider against the cluster's
		// SCRAM creds; Redpanda already provisioned topics inside NewRedpanda
		// using the same Kafka-protocol provider, so skip duplicate creation.
		if needsAWSStreamingStages {
			if err = aws.NewKafkaTopics(ctx, streamOut); err != nil {
				return err
			}
		}
		cicdOut, err = aws.NewCICD(ctx, cfg)
	default:
		return unsupportedCloud(cfg.CloudProvider)
	}
	if err != nil {
		return err
	}

	// =====================================================================
	// Stage 5: Compute (cluster, services, M4b, schema registry)
	// =====================================================================
	var (
		computeOut    types.ComputeOutputs
		svcOut        *compute.ServicesOutputs
		schemaUrl     pulumi.StringOutput
		schemaSvcName pulumi.StringOutput
	)
	switch cfg.CloudProvider {
	case "aws":
		computeOut, svcOut, err = aws.NewCompute(ctx, cfg, netOut, cicdOut, secretsOut)
		if err != nil {
			return err
		}
		// Redpanda ships its own Schema Registry (URL already populated in
		// streamOut.SchemaRegistryUrl by NewRedpanda) — only deploy the
		// standalone Confluent Schema Registry on ECS for MSK tenants.
		if needsAWSStreamingStages {
			schemaUrl, schemaSvcName, err = aws.NewSchemaRegistry(ctx, cfg, netOut, computeOut, streamOut, secretsOut)
		}
	default:
		return unsupportedCloud(cfg.CloudProvider)
	}
	if err != nil {
		return err
	}
	if needsAWSStreamingStages {
		streamOut.SchemaRegistryUrl = schemaUrl
	}

	// =====================================================================
	// Stage 6: Edge + Observability + HealthGate + Autoscaling
	// =====================================================================
	var edgeOut types.EdgeOutputs
	switch cfg.CloudProvider {
	case "aws":
		var tgOut *loadbalancer.TargetGroupOutputs
		edgeOut, tgOut, err = aws.NewEdge(ctx, cfg, netOut, storageOut)
		if err != nil {
			return err
		}
		// Autoscaling's ALBRequestCountPerTarget metric needs the ALB ARN
		// suffix (e.g. "app/kaizen-dev-alb/50dc6c495c0c9188"), not the full
		// ARN — see types.EdgeOutputs.LoadBalancerArnSuffix.
		if err = aws.NewAutoscaling(ctx, cfg, computeOut, svcOut, edgeOut.LoadBalancerArnSuffix, tgOut); err != nil {
			return err
		}
		if err = aws.NewObservability(ctx, cfg, dbOut, streamOut, computeOut); err != nil {
			return err
		}
		// Schema Registry health gate watches the standalone ECS service —
		// not applicable when Redpanda's built-in registry is in use.
		if needsAWSStreamingStages {
			if err = aws.NewKafkaHealthGate(ctx, cfg, computeOut, schemaSvcName); err != nil {
				return err
			}
		}
	default:
		return unsupportedCloud(cfg.CloudProvider)
	}

	// =====================================================================
	// Generic stack exports (cloud-agnostic strings)
	// =====================================================================
	ctx.Export("databaseEndpoint", dbOut.Endpoint)
	ctx.Export("cacheEndpoint", cacheOut.Endpoint)
	ctx.Export("streamingBootstrapBrokers", streamOut.BootstrapBrokers)
	// Cloud-agnostic Schema Registry URL: populated by aws.NewSchemaRegistry
	// (ECS Confluent) for MSK or by cloudstreaming.NewRedpanda (built-in
	// Redpanda registry) for Redpanda. Downstream consumers read this single
	// export regardless of which streaming provider is in use.
	ctx.Export("schemaRegistryUrl", streamOut.SchemaRegistryUrl)
	ctx.Export("loadBalancerDns", edgeOut.LoadBalancerDns)
	ctx.Export("dataBucket", storageOut.DataBucketName)

	return nil
}

func unsupportedCloud(provider string) error {
	switch provider {
	case "gcp":
		// GCP supports network, storage, data stores, streaming, secrets,
		// cicd, compute (M4b + Cloud Run), and edge (#496). GCP should never
		// reach the shared stage switches because Deploy() early-returns
		// after the edge block. Hitting this is a programming error: a stage
		// switch is missing a gcp case or the early-return was removed
		// without wiring the remaining stages (observability, #497).
		return fmt.Errorf("internal: cloudProvider=gcp reached an unimplemented stage (GCP wires Network + Storage + Data Stores + Streaming + Secrets + CICD + Compute + Edge; Observability #497 pending)")
	default:
		return fmt.Errorf("unsupported cloudProvider %q (expected \"aws\" or \"gcp\")", provider)
	}
}

func main() {
	pulumi.Run(Deploy)
}
