// Package database provisions Cloud SQL PostgreSQL for the Kaizen
// experimentation platform on GCP. Configured for parity with pkg/aws/database
// (RDS PostgreSQL 16): regional HA in staging/prod, daily backups with PITR,
// matching maintenance window and 7-day backup retention, IAM-DB authentication
// enabled, private IP only (reachable via the VPC through Private Service
// Access).
//
// The instance does not provision a built-in admin user. Operators authenticate
// via IAM database auth (the `cloud_sql.iam_authentication` database flag is
// set to `on`), with specific service-account → DB user bindings created
// alongside compute (#486). The DatabaseSecret entry in pkg/gcp/secrets
// remains as a placeholder for service-account credentials; password-based
// access is intentionally not configured at provisioning time.
package database

import (
	"fmt"

	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/sql"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
)

// CloudSQLOutputs holds outputs that downstream GCP modules (compute, secrets)
// consume. The facade adapts these into the cross-cloud types.DatabaseOutputs.
type CloudSQLOutputs struct {
	// Endpoint is host:port form (e.g., "10.99.0.3:5432"), matching what the
	// AWS RDS endpoint output produces. Composed from the private IP and the
	// PostgreSQL default port.
	Endpoint pulumi.StringOutput

	// Port is the listener port (always 5432 for PostgreSQL).
	Port pulumi.IntOutput

	// InstanceId is the bare Cloud SQL instance name, used by ops tooling
	// (gcloud sql connect, gcloud sql describe).
	InstanceId pulumi.StringOutput

	// PrivateIp is the assigned RFC1918 address. Exported so the secrets
	// module can populate DatabaseSecret.Host without round-tripping through
	// the Endpoint string parser.
	PrivateIp pulumi.StringOutput
}

// CloudSQLInputs carries the wiring from upstream stages. PrivateNetwork is
// the VPC self-link (NetworkOutputs.VpcId on GCP). PsaReservedRangeName flows
// in from the same source — its presence here creates an implicit Pulumi
// dependency on the Private Service Access peering so the instance is not
// created before the peering is live.
type CloudSQLInputs struct {
	// PrivateNetwork is the VPC self-link the Cloud SQL instance peers into.
	PrivateNetwork pulumi.StringInput

	// PsaReservedRangeName is the name of the Private Service Access reserved
	// range. Attached as a UserLabel so Pulumi's output graph orders this
	// instance after the PSA peering completes.
	PsaReservedRangeName pulumi.StringInput
}

// NewCloudSQL creates a Cloud SQL for PostgreSQL 16 instance with the
// configuration matching pkg/aws/database/rds.go. Sizing, HA, backup, and
// maintenance windows are read from the same kaizen-experimentation namespace
// as the AWS module; provider-specific keys live under the gcpDb* prefix.
func NewCloudSQL(ctx *pulumi.Context, kcfg *kconfig.Config, inputs *CloudSQLInputs) (*CloudSQLOutputs, error) {
	if inputs == nil || inputs.PrivateNetwork == nil {
		return nil, fmt.Errorf("gcp/database.NewCloudSQL: PrivateNetwork input is required (pass NetworkOutputs.VpcId)")
	}

	cfg := config.New(ctx, "kaizen-experimentation")

	// Region: prefer the app-level gcpRegion (consistent with cache and
	// compute modules which read cfg.GCPRegion) then fall back to the
	// provider-level gcp:region, then to the hard-coded default.
	region := kcfg.GCPRegion
	if region == "" {
		gcpCfg := config.New(ctx, "gcp")
		region = gcpCfg.Get("region")
	}
	if region == "" {
		region = "us-central1"
	}

	// --- Tier (machine type) ---
	// Parity with AWS RDS sizing:
	//   dev:            db.t4g.medium   →  db-custom-2-4096   (2 vCPU, 4 GB)
	//   staging/prod:   db.r6g.large    →  db-custom-4-16384  (4 vCPU, 16 GB)
	tier := cfg.Get("gcpDbTier")
	if tier == "" {
		if kcfg.IsProd() || kcfg.IsStaging() {
			tier = "db-custom-4-16384"
		} else {
			tier = "db-custom-2-4096"
		}
	}

	// --- Availability type ---
	// Cloud SQL's REGIONAL availability is the analog of RDS MultiAz: the
	// instance fails over to a standby in another zone within the region.
	availability := "ZONAL"
	if kcfg.IsProd() || kcfg.IsStaging() {
		availability = "REGIONAL"
	}
	if v := cfg.Get("gcpDbAvailabilityType"); v != "" {
		availability = v
	}

	// --- Deletion protection ---
	// Prod-only, matching RDS. Non-prod stacks can be torn down freely.
	deletionProtection := kcfg.IsProd()

	// --- Disk sizing ---
	// AWS RDS: AllocatedStorage=100 GB, MaxAllocatedStorage=500 GB, gp3, encrypted.
	// Cloud SQL: DiskSize=100, DiskAutoresize=true, DiskAutoresizeLimit=500,
	// DiskType=PD_SSD. Disk-level encryption with Google-managed KMS is on by
	// default (CMEK opt-in is a follow-up).
	diskSize := 100
	if v, err := cfg.TryInt("gcpDbDiskSizeGb"); err == nil {
		diskSize = v
	}
	diskAutoresizeLimit := 500
	if v, err := cfg.TryInt("gcpDbDiskAutoresizeLimitGb"); err == nil {
		diskAutoresizeLimit = v
	}

	// --- Instance name ---
	// Cloud SQL instance names are unique within a project and CANNOT be
	// reused for 7 days after deletion — so we tie the name to the env to
	// reduce churn during day-1 stack iteration.
	instanceName := fmt.Sprintf("kaizen-sql-%s", kcfg.Environment)

	// --- Resource labels carrying PSA dependency ---
	// Cloud SQL doesn't reference the PSA range directly via API, but Pulumi's
	// resource graph needs *some* data-flow link from the peering output to
	// this instance so creation order is enforced. Attaching the range name as
	// a user label gives us that edge while also recording the provisioning
	// link for ops auditing.
	labels := pulumi.StringMap{
		"project":     pulumi.String("kaizen"),
		"environment": pulumi.String(kcfg.Environment),
		"managed_by":  pulumi.String("pulumi"),
	}
	if inputs.PsaReservedRangeName != nil {
		labels["psa-range"] = inputs.PsaReservedRangeName.ToStringOutput()
	}

	// --- Cloud SQL instance ---
	instance, err := sql.NewDatabaseInstance(ctx, instanceName, &sql.DatabaseInstanceArgs{
		Name:               pulumi.String(instanceName),
		DatabaseVersion:    pulumi.String("POSTGRES_16"),
		Region:             pulumi.String(region),
		DeletionProtection: pulumi.Bool(deletionProtection),
		Settings: &sql.DatabaseInstanceSettingsArgs{
			Tier:                pulumi.String(tier),
			AvailabilityType:    pulumi.String(availability),
			DiskSize:            pulumi.Int(diskSize),
			DiskType:            pulumi.String("PD_SSD"),
			DiskAutoresize:      pulumi.Bool(true),
			DiskAutoresizeLimit: pulumi.Int(diskAutoresizeLimit),
			UserLabels:          labels,

			IpConfiguration: &sql.DatabaseInstanceSettingsIpConfigurationArgs{
				// Public IP disabled — the instance is only reachable via the
				// VPC peering established by Private Service Access.
				Ipv4Enabled:    pulumi.Bool(false),
				PrivateNetwork: inputs.PrivateNetwork,
			},

			BackupConfiguration: &sql.DatabaseInstanceSettingsBackupConfigurationArgs{
				Enabled:                     pulumi.Bool(true),
				PointInTimeRecoveryEnabled:  pulumi.Bool(true),
				StartTime:                   pulumi.String("03:00"),
				TransactionLogRetentionDays: pulumi.Int(7),
				BackupRetentionSettings: &sql.DatabaseInstanceSettingsBackupConfigurationBackupRetentionSettingsArgs{
					RetainedBackups: pulumi.Int(7),
					RetentionUnit:   pulumi.String("COUNT"),
				},
			},

			MaintenanceWindow: &sql.DatabaseInstanceSettingsMaintenanceWindowArgs{
				// Cloud SQL day-of-week is 1=Monday..7=Sunday. Sunday 05:00 UTC
				// matches the RDS `sun:05:00-sun:06:00` maintenance window.
				Day:         pulumi.Int(7),
				Hour:        pulumi.Int(5),
				UpdateTrack: pulumi.String("stable"),
			},

			DatabaseFlags: sql.DatabaseInstanceSettingsDatabaseFlagArray{
				&sql.DatabaseInstanceSettingsDatabaseFlagArgs{
					Name:  pulumi.String("cloud_sql.iam_authentication"),
					Value: pulumi.String("on"),
				},
			},
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating Cloud SQL instance: %w", err)
	}

	// --- Application database ---
	// Matches the AWS RDS module's DbName: "kaizen".
	if _, err = sql.NewDatabase(ctx, "kaizen-sql-db", &sql.DatabaseArgs{
		Name:     pulumi.String("kaizen"),
		Instance: instance.Name,
	}); err != nil {
		return nil, fmt.Errorf("creating Cloud SQL database: %w", err)
	}

	// Endpoint composition: Cloud SQL exposes PrivateIpAddress; combine with
	// 5432 to produce the host:port shape the cross-cloud DatabaseOutputs
	// contract requires.
	endpoint := pulumi.Sprintf("%s:5432", instance.PrivateIpAddress)

	return &CloudSQLOutputs{
		Endpoint:   endpoint,
		Port:       pulumi.Int(5432).ToIntOutput(),
		InstanceId: instance.Name,
		PrivateIp:  instance.PrivateIpAddress,
	}, nil
}
