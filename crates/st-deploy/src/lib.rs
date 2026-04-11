//! Program bundler and deployment target configuration.
//!
//! This crate provides:
//! - **Target configuration**: parse `targets:` from `plc-project.yaml`
//! - **Program bundling**: compile a project and package it into a `.st-bundle`
//!   archive (tar.gz) for deployment to remote targets
//! - **Bundle verification**: SHA-256 checksums and manifest validation

pub mod bundle;
pub mod debug_info;
pub mod installer;
pub mod ssh;
pub mod target;

pub use bundle::{BundleManifest, BundleMode, ProgramBundle};
pub use debug_info::DebugMap;
pub use installer::{InstallOptions, InstallResult};
pub use ssh::SshTarget;
pub use target::{AuthMode, Target, TargetConfig};
