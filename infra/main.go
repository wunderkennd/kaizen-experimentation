package main

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/aws"
	"github.com/kaizen-experimentation/infra/pkg/aws/loadbalancer"
	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
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
		// Autoscaling references the ALB ARN suffix; the underlying ALBOutputs
		// exposes it as albOut.AlbArnSuffix. The aggregator returns the full
		// ARN in EdgeOutputs.LoadBalancerArn — for behavior parity with the
		// original main.go, the autoscaling helper accepts the StringOutput
		// directly and the compute module slices it as needed.
		if err = aws.NewAutoscaling(ctx, cfg, computeOut, edgeOut.LoadBalancerArn, tgOut); err != nil {
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
		return fmt.Errorf("cloudProvider=gcp is not implemented yet (Phase 1 of ADR multi-cloud foundation)")
	default:
		return fmt.Errorf("unsupported cloudProvider %q (expected \"aws\" or \"gcp\")", provider)
	}
}

func main() {
	pulumi.Run(Deploy)
}
