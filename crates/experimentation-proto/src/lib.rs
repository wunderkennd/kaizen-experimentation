//! Auto-generated Protobuf types and gRPC service definitions.
//!
//! This crate contains the Rust types generated from the proto/ directory.
//! Do NOT manually edit generated code — modify the .proto files instead.

pub mod experimentation {
    pub mod common {
        pub mod v1 {
            tonic::include_proto!("experimentation.common.v1");
        }
    }
    pub mod pipeline {
        pub mod v1 {
            tonic::include_proto!("experimentation.pipeline.v1");
        }
    }
    pub mod assignment {
        pub mod v1 {
            tonic::include_proto!("experimentation.assignment.v1");
        }
    }
    pub mod bandit {
        pub mod v1 {
            tonic::include_proto!("experimentation.bandit.v1");
        }
    }
    pub mod analysis {
        pub mod v1 {
            tonic::include_proto!("experimentation.analysis.v1");
        }
    }
    pub mod management {
        pub mod v1 {
            tonic::include_proto!("experimentation.management.v1");
        }
    }
    pub mod metrics {
        pub mod v1 {
            tonic::include_proto!("experimentation.metrics.v1");
        }
    }
    pub mod flags {
        pub mod v1 {
            tonic::include_proto!("experimentation.flags.v1");
        }
    }
}

// Convenience re-exports
pub use experimentation::common::v1 as common;
pub use experimentation::pipeline::v1 as pipeline;
