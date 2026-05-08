package main

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/aws"
	"github.com/kaizen-experimentation/infra/pkg/aws/loadbalancer"
	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp"
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
	// GCP Phase 1: CICD-only slice
	// =====================================================================
	// Phase 1 ships only Artifact Registry. Network/storage/database/etc.
	// land in subsequent phases per docs/superpowers/specs/2026-04-20-multi-cloud-gcp-aws-design.md.
	// Until they do, a `cloudProvider=gcp` stack runs only the CICD stage and
	// returns — that's exactly what `pulumi preview --stack gcp-dev` needs to
	// succeed against the issue #482 acceptance criterion.
	if cfg.CloudProvider == "gcp" {
		cicdOut, err := gcp.NewCICD(ctx, cfg)
		if err != nil {
			return err
		}
		// Export a representative URL so `pulumi stack output` is non-empty.
		if url, ok := cicdOut.RepositoryURLs["assignment"]; ok {
			ctx.Export("cicdAssignmentRepositoryUrl", url)
		}
		return nil
	}

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
	default:
		return fmt.Errorf("unsupported streamingProvider %q (Phase 0 supports only \"msk\")", cfg.StreamingProvider)
	}
	if err != nil {
		return err
	}
	switch cfg.CloudProvider {
	case "aws":
		secretsOut, err = aws.NewSecrets(ctx, cfg, dbOut, streamOut, cacheOut)
		if err != nil {
			return err
		}
		if err = aws.NewKafkaTopics(ctx, streamOut); err != nil {
			return err
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
		schemaUrl, schemaSvcName, err = aws.NewSchemaRegistry(ctx, cfg, netOut, computeOut, streamOut, secretsOut)
	default:
		return unsupportedCloud(cfg.CloudProvider)
	}
	if err != nil {
		return err
	}
	streamOut.SchemaRegistryUrl = schemaUrl

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
		if err = aws.NewKafkaHealthGate(ctx, cfg, computeOut, schemaSvcName); err != nil {
			return err
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
	ctx.Export("loadBalancerDns", edgeOut.LoadBalancerDns)
	ctx.Export("dataBucket", storageOut.DataBucketName)

	return nil
}

func unsupportedCloud(provider string) error {
	switch provider {
	case "gcp":
		// Phase 1 only supports the cicd slice on GCP — that path returns
		// before reaching any of the other stage switches. Hitting this is
		// a programming error: a switch added a gcp case but Deploy() should
		// have early-returned above.
		return fmt.Errorf("internal: cloudProvider=gcp reached a non-CICD stage (Phase 1 supports only Artifact Registry)")
	default:
		return fmt.Errorf("unsupported cloudProvider %q (expected \"aws\" or \"gcp\")", provider)
	}
}

func main() {
	pulumi.Run(Deploy)
}
