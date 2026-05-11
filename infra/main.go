package main

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/aws"
	"github.com/kaizen-experimentation/infra/pkg/aws/loadbalancer"
	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp"
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
	// GCP early return — Phase 1 storage + cicd slice
	// =====================================================================
	// Stages 3–6 (data stores, streaming, compute, edge) are AWS-only today.
	// GCP arms for those stages land in subsequent Phase 1 PRs. Until they do,
	// we run the two implemented GCP stages (storage above, cicd below) and
	// return cleanly so `pulumi preview --stack gcp-dev` succeeds.
	// Each subsequent Phase 1 PR moves this early-return marker further down
	// Deploy() and removes it entirely once all stages are wired.
	if cfg.CloudProvider == "gcp" {
		cicdOut, err := gcp.NewCICD(ctx, cfg)
		if err != nil {
			return err
		}
		if url, ok := cicdOut.RepositoryURLs["assignment"]; ok {
			ctx.Export("cicdAssignmentRepositoryUrl", url)
		}
		ctx.Export("dataBucket", storageOut.DataBucketName)
		return nil
	}

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
	default:
		return unsupportedCloud(cfg.CloudProvider)
	}
	if err != nil {
		return err
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
		schemaUrl     pulumi.StringOutput
		schemaSvcName pulumi.StringOutput
	)
	switch cfg.CloudProvider {
	case "aws":
		computeOut, _, err = aws.NewCompute(ctx, cfg, netOut, cicdOut, secretsOut)
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
		if err = aws.NewAutoscaling(ctx, cfg, computeOut, edgeOut.LoadBalancerArnSuffix, tgOut); err != nil {
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
		// Phase 1 supports storage (Stage 2) and cicd (early-return block) on GCP.
		// Stages 3–6 are AWS-only today; GCP should never reach their switches
		// because Deploy() early-returns after the cicd block. Hitting this is
		// a programming error: a stage switch is missing a gcp case or the
		// early-return was removed without wiring the remaining stages.
		return fmt.Errorf("internal: cloudProvider=gcp reached an unimplemented stage (Phase 1 supports Storage + CICD only)")
	default:
		return fmt.Errorf("unsupported cloudProvider %q (expected \"aws\" or \"gcp\")", provider)
	}
}

func main() {
	pulumi.Run(Deploy)
}
