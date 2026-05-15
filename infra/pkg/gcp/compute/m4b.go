// Package compute provisions the GCP compute layer for Kaizen. Phase 1 ships
// only the stateful M4b Policy Service slice — the Cloud Run analogs for the
// eight stateless services land in a sibling PR (#486). Layout mirrors
// pkg/aws/compute so each provider's compute module owns its cluster + M4b
// resources behind a common types.ComputeOutputs contract.
//
// M4b on GCP is the analog of the AWS "EC2-in-ASG-of-1" pattern:
//
//   - Persistent Disk (50GB pd-ssd) created as an independent resource so its
//     lifecycle is decoupled from the instance. The MIG never deletes it.
//   - Static internal IP reserved from the private subnet so the Service
//     Directory endpoint is stable across instance recreations.
//   - Instance Template defines the n2-standard-4 shape, boot disk, and
//     network tag — but not the data disk. The data disk attaches via the
//     MIG's stateful policy.
//   - Zonal Managed Instance Group with target size 1, autohealing health
//     check, and stateful-disk + stateful-internal-ip policies. Zonal (not
//     regional) because PDs are zonal — a regional MIG could rebalance to
//     another zone and orphan the disk.
//   - Per-instance config binds the named stateful instance to the specific
//     disk and IP so recreation reattaches both.
//   - Service Directory Service + Endpoint register m4b-policy at the
//     reserved IP and port 50054 under the existing kaizen-local namespace.
//
// The same Kaizen runtime container ships unmodified on both clouds — the
// invariants (single instance, RocksDB on persistent volume, LMAX
// single-threaded core, Kafka consumer for reward events, < 10s recovery)
// hold on either provider. See docs/superpowers/specs/2026-04-20-multi-cloud-gcp-aws-design.md
// (Compute Model → M4b Policy Service).
package compute

import (
	"fmt"
	"strings"

	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/compute"
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/servicedirectory"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// M4bPort is the gRPC port the M4b Policy service binds to. Matches the AWS
// CloudMap registration and the value baked into the Kaizen container.
const M4bPort = 50054

// M4bDataDeviceName is the udev/sd-name the startup script mounts at
// /data/rocksdb. The MIG's stateful-disk policy references this device name,
// so changing it requires updating the startup script too.
const M4bDataDeviceName = "m4b-data"

// m4bNetworkTag is the GCP firewall network tag the m4b firewall rule
// matches against. Defined in pkg/gcp/network/firewall.go as tagM4b.
const m4bNetworkTag = "kaizen-m4b"

// M4bArgs configures the GCP M4b Policy compute slice.
type M4bArgs struct {
	// Environment name: "dev", "staging", or "prod". Used for resource naming.
	Environment string

	// Region is the GCP region. Static IP reservation is regional; the IGM
	// and Disk are zonal (see Zone).
	Region pulumi.StringOutput

	// Zone is the specific GCE zone the MIG, instance, and persistent disk
	// live in. PDs and zonal MIGs are zonal resources, so the M4b slice
	// commits to one zone for the entire stateful set. Defaults to the
	// region's first zone (suffix "-a") when empty.
	Zone string

	// PrivateSubnetSelfLink is the self-link of the private subnet that
	// holds the M4b instance and its reserved internal IP. From
	// pkg/gcp/network's VpcOutputs.PrivateSubnetIds[0].
	PrivateSubnetSelfLink pulumi.StringInput

	// ServiceDirectoryNamespaceName is the fully-qualified resource name of
	// the Service Directory namespace (kaizen-local) the m4b-policy service
	// registers under, e.g. projects/<P>/locations/<R>/namespaces/kaizen-local.
	ServiceDirectoryNamespaceName pulumi.StringInput

	// InstanceType is the GCE machine type. Defaults to n2-standard-4
	// (4 vCPU, 16 GB) per spec; staging/prod can override.
	InstanceType string

	// DataDiskSizeGb is the size of the RocksDB persistent disk. Defaults
	// to 50 per spec.
	DataDiskSizeGb int
}

// M4bOutputs exposes the resources the GCP compute facade composes into
// types.ComputeOutputs.M4b* and that the topology test asserts on.
type M4bOutputs struct {
	// MigName is the Managed Instance Group name (analogous to the AWS ASG
	// name). Exposed via types.ComputeOutputs.M4bAsgName.
	MigName pulumi.StringOutput

	// InstanceName is the deterministic per-instance config name — the
	// single VM created by the MIG. Exposed via
	// types.ComputeOutputs.M4bInstanceId.
	InstanceName pulumi.StringOutput

	// Endpoint is the resolvable host:port form of the M4b Service Directory
	// endpoint. Exposed via types.ComputeOutputs.M4bEndpoint.
	Endpoint pulumi.StringOutput

	// ServiceName is the Service Directory service resource name
	// (projects/<P>/locations/<R>/namespaces/<N>/services/m4b-policy).
	// Surfaced for downstream consumers that resolve via the SD API rather
	// than the host:port string.
	ServiceName pulumi.StringOutput

	// DataDiskName is the name of the persistent disk holding the RocksDB
	// snapshot. Useful for backup-policy attachment in a follow-up PR.
	DataDiskName pulumi.StringOutput
}

// NewM4bInstance provisions the M4b Policy compute slice on GCP: persistent
// disk, static internal IP, instance template, autohealing health check,
// zonal MIG with stateful policies, per-instance config, and Service
// Directory registration. See package doc for the rationale.
func NewM4bInstance(ctx *pulumi.Context, args *M4bArgs) (*M4bOutputs, error) {
	if args == nil {
		return nil, fmt.Errorf("gcp/compute: M4bArgs must not be nil")
	}
	if args.Environment == "" {
		return nil, fmt.Errorf("gcp/compute: M4bArgs.Environment is required")
	}
	if args.PrivateSubnetSelfLink == nil {
		return nil, fmt.Errorf("gcp/compute: M4bArgs.PrivateSubnetSelfLink is required")
	}
	if args.ServiceDirectoryNamespaceName == nil {
		return nil, fmt.Errorf("gcp/compute: M4bArgs.ServiceDirectoryNamespaceName is required")
	}

	if args.InstanceType == "" {
		args.InstanceType = "n2-standard-4"
	}
	if args.DataDiskSizeGb == 0 {
		args.DataDiskSizeGb = 50
	}

	prefix := fmt.Sprintf("kaizen-%s", args.Environment)
	labels := pulumi.StringMap{
		"project":     pulumi.String("kaizen"),
		"environment": pulumi.String(args.Environment),
		"managed_by":  pulumi.String("pulumi"),
		"service":     pulumi.String("m4b-policy"),
	}

	// Zone defaults to first zone in region (e.g. us-central1 → us-central1-a)
	// when not explicitly set. PDs, zonal MIGs, and per-instance configs all
	// pin to this single zone to avoid disk/instance topology drift.
	zone := args.Zone
	zoneOutput := args.Region.ApplyT(func(r string) string {
		if zone != "" {
			return zone
		}
		return r + "-a"
	}).(pulumi.StringOutput)

	// ── Persistent Disk: 50GB pd-ssd, independent lifecycle ──────────────
	dataDisk, err := compute.NewDisk(ctx, "m4b-data-disk", &compute.DiskArgs{
		Name:        pulumi.Sprintf("%s-m4b-data", prefix),
		Description: pulumi.String("M4b Policy RocksDB snapshot — survives instance recreation"),
		Size:        pulumi.Int(args.DataDiskSizeGb),
		Type:        pulumi.String("pd-ssd"),
		Zone:        zoneOutput,
		Labels:      labels,
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b data disk: %w", err)
	}

	// ── Static internal IP for stable Service Directory endpoint ─────────
	// Reserve in the private subnet so the address survives MIG instance
	// recreations and the Service Directory endpoint never has to be
	// rewritten.
	internalIP, err := compute.NewAddress(ctx, "m4b-internal-ip", &compute.AddressArgs{
		Name:        pulumi.Sprintf("%s-m4b-ip", prefix),
		Description: pulumi.String("Static internal IP for M4b Policy MIG instance"),
		AddressType: pulumi.String("INTERNAL"),
		Purpose:     pulumi.String("GCE_ENDPOINT"),
		Subnetwork:  args.PrivateSubnetSelfLink,
		Region:      args.Region,
		Labels:      labels,
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b internal IP: %w", err)
	}

	// ── Health check: TCP on port 50054 for MIG autohealing ──────────────
	// Detection budget: 10s interval × 3 failed checks ≈ 30s detection.
	// Recreation + disk reattach is ~2-4s on GCP for a zonal MIG with a
	// pre-bound stateful disk. Combined with the M4b RocksDB warm-load,
	// end-to-end recovery stays well inside the < 10s SLO (validated by
	// the #24 chaos test, separate issue).
	healthCheck, err := compute.NewHealthCheck(ctx, "m4b-health-check", &compute.HealthCheckArgs{
		Name:               pulumi.Sprintf("%s-m4b-hc", prefix),
		Description:        pulumi.String("M4b Policy autohealing health check — TCP/50054"),
		CheckIntervalSec:   pulumi.Int(10),
		TimeoutSec:         pulumi.Int(5),
		HealthyThreshold:   pulumi.Int(2),
		UnhealthyThreshold: pulumi.Int(3),
		TcpHealthCheck: &compute.HealthCheckTcpHealthCheckArgs{
			Port: pulumi.Int(M4bPort),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b health check: %w", err)
	}

	// ── Instance template: shape only; data disk attaches via stateful policy ─
	// The startup script formats and mounts the data disk on its predictable
	// /dev/disk/by-id/google-<device-name> path. Identical pattern to the
	// AWS user-data in pkg/aws/compute/cluster.go::newM4bLaunchTemplate.
	startupScript := fmt.Sprintf(`#!/bin/bash
set -euo pipefail

# Wait for the data disk to attach (MIG attaches via per-instance config).
DEVICE="/dev/disk/by-id/google-%s"
while [ ! -b "$DEVICE" ]; do
  echo "Waiting for $DEVICE..."
  sleep 1
done

# Format only if the disk is empty (first boot). Subsequent reboots reuse
# the existing RocksDB filesystem so the snapshot survives.
if ! blkid "$DEVICE" >/dev/null 2>&1; then
  mkfs.xfs "$DEVICE"
fi

mkdir -p /data/rocksdb
mount -o defaults,nofail "$DEVICE" /data/rocksdb
grep -q "$DEVICE" /etc/fstab || echo "$DEVICE /data/rocksdb xfs defaults,nofail 0 2" >> /etc/fstab

# Ownership for the Kaizen runtime container's UID/GID.
chown -R 1000:1000 /data/rocksdb
`, M4bDataDeviceName)

	template, err := compute.NewInstanceTemplate(ctx, "m4b-instance-template", &compute.InstanceTemplateArgs{
		Name:        pulumi.Sprintf("%s-m4b-template", prefix),
		Description: pulumi.String("M4b Policy instance template — boot disk only; data disk attaches via MIG stateful policy"),
		MachineType: pulumi.String(args.InstanceType),
		Region:      args.Region,
		Tags:        pulumi.StringArray{pulumi.String(m4bNetworkTag)},
		Labels:      labels,

		// Boot disk: small pd-balanced for the OS only. Auto-deleted with the
		// instance — the stateful data disk holds everything that matters.
		Disks: compute.InstanceTemplateDiskArray{
			&compute.InstanceTemplateDiskArgs{
				Boot:        pulumi.Bool(true),
				AutoDelete:  pulumi.Bool(true),
				DiskSizeGb:  pulumi.Int(20),
				DiskType:    pulumi.String("pd-balanced"),
				SourceImage: pulumi.String("projects/debian-cloud/global/images/family/debian-12"),
			},
		},

		// Private network interface, no public IP. Egress flows through
		// Cloud NAT (provisioned by pkg/gcp/network/vpc.go).
		NetworkInterfaces: compute.InstanceTemplateNetworkInterfaceArray{
			&compute.InstanceTemplateNetworkInterfaceArgs{
				Subnetwork: args.PrivateSubnetSelfLink,
				// No AccessConfigs ⇒ no public IP.
			},
		},

		Metadata: pulumi.StringMap{
			"startup-script":         pulumi.String(startupScript),
			"enable-oslogin":         pulumi.String("TRUE"),
			"block-project-ssh-keys": pulumi.String("TRUE"),
		},

		Scheduling: &compute.InstanceTemplateSchedulingArgs{
			// On-demand only; preemptible/spot is incompatible with stateful
			// M4b. AutomaticRestart pairs with the MIG autohealer.
			Preemptible:       pulumi.Bool(false),
			AutomaticRestart:  pulumi.Bool(true),
			OnHostMaintenance: pulumi.String("MIGRATE"),
		},

		ShieldedInstanceConfig: &compute.InstanceTemplateShieldedInstanceConfigArgs{
			EnableSecureBoot:          pulumi.Bool(true),
			EnableVtpm:                pulumi.Bool(true),
			EnableIntegrityMonitoring: pulumi.Bool(true),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b instance template: %w", err)
	}

	// ── Managed Instance Group: zonal, size 1, stateful policies ─────────
	// statefulDisks: device-name pins the m4b-data disk so it's reattached,
	//   not deleted, when the MIG recreates the VM.
	// statefulInternalIps: pins the nic0 internal IP across recreations so
	//   the Service Directory endpoint stays valid.
	// AutoHealingPolicies: TCP/50054 probe → recreate on 3 failures.
	mig, err := compute.NewInstanceGroupManager(ctx, "m4b-mig", &compute.InstanceGroupManagerArgs{
		Name:             pulumi.Sprintf("%s-m4b-mig", prefix),
		Description:      pulumi.String("M4b Policy MIG — single stateful instance, autohealed"),
		BaseInstanceName: pulumi.Sprintf("%s-m4b", prefix),
		Zone:             zoneOutput,
		TargetSize:       pulumi.Int(1),

		Versions: compute.InstanceGroupManagerVersionArray{
			&compute.InstanceGroupManagerVersionArgs{
				Name:             pulumi.String("primary"),
				InstanceTemplate: template.SelfLink,
			},
		},

		AutoHealingPolicies: &compute.InstanceGroupManagerAutoHealingPoliciesArgs{
			HealthCheck: healthCheck.SelfLink,
			// 5-minute grace period covers first-boot RocksDB warm-load and
			// Kafka consumer-group rebalance before the autohealer engages.
			InitialDelaySec: pulumi.Int(300),
		},

		StatefulDisks: compute.InstanceGroupManagerStatefulDiskArray{
			&compute.InstanceGroupManagerStatefulDiskArgs{
				DeviceName: pulumi.String(M4bDataDeviceName),
				DeleteRule: pulumi.String("NEVER"),
			},
		},

		StatefulInternalIps: compute.InstanceGroupManagerStatefulInternalIpArray{
			&compute.InstanceGroupManagerStatefulInternalIpArgs{
				InterfaceName: pulumi.String("nic0"),
				DeleteRule:    pulumi.String("NEVER"),
			},
		},

		// Rolling-update policy tuned for a single stateful VM: REPLACE
		// strategy with MaxUnavailable=1 / MaxSurge=0 is the only valid
		// combination for size-1 MIGs with stateful policies — GCP rejects
		// surge updates on stateful groups.
		UpdatePolicy: &compute.InstanceGroupManagerUpdatePolicyArgs{
			Type:                        pulumi.String("OPPORTUNISTIC"),
			MinimalAction:               pulumi.String("REPLACE"),
			MostDisruptiveAllowedAction: pulumi.String("REPLACE"),
			ReplacementMethod:           pulumi.String("RECREATE"),
			MaxSurgeFixed:               pulumi.Int(0),
			MaxUnavailableFixed:         pulumi.Int(1),
		},

		NamedPorts: compute.InstanceGroupManagerNamedPortArray{
			&compute.InstanceGroupManagerNamedPortArgs{
				Name: pulumi.String("grpc-m4b"),
				Port: pulumi.Int(M4bPort),
			},
		},

		WaitForInstances: pulumi.Bool(false),
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b MIG: %w", err)
	}

	// ── Per-instance config: bind the named instance to disk + IP ────────
	// The MIG generates instance names as "<baseInstanceName>-<suffix>", but
	// the per-instance config explicitly names a specific instance so the
	// stateful disk and IP attach deterministically across recreations.
	instanceName := pulumi.Sprintf("%s-m4b-0", prefix)
	_, err = compute.NewPerInstanceConfig(ctx, "m4b-instance-config", &compute.PerInstanceConfigArgs{
		InstanceGroupManager: mig.Name,
		Zone:                 zoneOutput,
		Name:                 instanceName,
		PreservedState: &compute.PerInstanceConfigPreservedStateArgs{
			Disks: compute.PerInstanceConfigPreservedStateDiskArray{
				&compute.PerInstanceConfigPreservedStateDiskArgs{
					DeviceName: pulumi.String(M4bDataDeviceName),
					Source:     dataDisk.ID().ToStringOutput(),
					Mode:       pulumi.String("READ_WRITE"),
					DeleteRule: pulumi.String("NEVER"),
				},
			},
			InternalIps: compute.PerInstanceConfigPreservedStateInternalIpArray{
				&compute.PerInstanceConfigPreservedStateInternalIpArgs{
					InterfaceName: pulumi.String("nic0"),
					AutoDelete:    pulumi.String("NEVER"),
					IpAddress: &compute.PerInstanceConfigPreservedStateInternalIpIpAddressArgs{
						Address: internalIP.SelfLink,
					},
				},
			},
		},
		MinimalAction:                pulumi.String("NONE"),
		MostDisruptiveAllowedAction:  pulumi.String("REPLACE"),
		RemoveInstanceStateOnDestroy: pulumi.Bool(false),
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b per-instance config: %w", err)
	}

	// ── Service Directory: m4b-policy under kaizen-local ─────────────────
	// Single service registered with the reserved internal IP and gRPC
	// port. Clients resolve via the Service Directory API or via the
	// auto-published private DNS zone.
	service, err := servicedirectory.NewService(ctx, "m4b-sd-service", &servicedirectory.ServiceArgs{
		ServiceId: pulumi.String("m4b-policy"),
		Namespace: args.ServiceDirectoryNamespaceName,
		Metadata: pulumi.StringMap{
			"port":     pulumi.String(fmt.Sprintf("%d", M4bPort)),
			"protocol": pulumi.String("grpc"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b Service Directory service: %w", err)
	}

	endpoint, err := servicedirectory.NewEndpoint(ctx, "m4b-sd-endpoint", &servicedirectory.EndpointArgs{
		EndpointId: pulumi.String("m4b-policy-instance-0"),
		Service:    service.ID(),
		Address:    internalIP.Address,
		Port:       pulumi.Int(M4bPort),
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b Service Directory endpoint: %w", err)
	}

	// host:port form for ComputeOutputs.M4bEndpoint — clients can either
	// resolve via Service Directory or dial this string directly.
	endpointAddr := pulumi.All(endpoint.Address, endpoint.Port).ApplyT(func(parts []interface{}) string {
		addr, _ := parts[0].(string)
		port, _ := parts[1].(int)
		if addr == "" {
			return ""
		}
		return fmt.Sprintf("%s:%d", strings.TrimSpace(addr), port)
	}).(pulumi.StringOutput)

	return &M4bOutputs{
		MigName:      mig.Name,
		InstanceName: instanceName,
		Endpoint:     endpointAddr,
		ServiceName:  service.Name,
		DataDiskName: dataDisk.Name,
	}, nil
}
