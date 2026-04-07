package main

import (
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kennethsylvain/kaizen-experimentation/infra/pkg/config"
)

func main() {
	pulumi.Run(func(ctx *pulumi.Context) error {
		cfg := config.LoadConfig(ctx)

		ctx.Export("environment", pulumi.String(cfg.Environment))

		// Sprint I.0 modules will be wired here as they land:
		//   network  := network.New(ctx, cfg)   // Infra-1
		//   database := database.New(ctx, cfg, network)  // Infra-2
		//   storage  := storage.New(ctx, cfg)   // Infra-2
		//   secrets  := secrets.New(ctx, cfg)   // Infra-2
		//   streaming := streaming.New(ctx, cfg, network) // Infra-3
		//   compute  := compute.New(ctx, cfg, network, database, streaming, secrets) // Infra-4
		//   loadbalancer.New(ctx, cfg, network, compute)  // Infra-5
		//   dns.New(ctx, cfg)                   // Infra-5
		//   observability.New(ctx, cfg, compute) // Infra-5

		return nil
	})
}
