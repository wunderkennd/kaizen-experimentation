package main

import (
	"github.com/kaizen-experimentation/infra/pkg/network"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

func main() {
	pulumi.Run(func(ctx *pulumi.Context) error {
		_, err := network.NewVpc(ctx)
		if err != nil {
			return err
		}
		return nil
	})
}
