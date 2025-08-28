//! Bot Configuration Management Crate
//! 
//! This crate provides a comprehensive configuration system for the Artemis liquidator bot,
//! including TOML file loading, environment variable expansion, and configuration validation.
//! 
//! # Features
//! 
//! - **TOML Configuration**: Load settings from TOML files
//! - **Environment Variables**: Secure secret management with env var expansion
//! - **Type Safety**: Strongly typed configuration structures
//! - **Validation**: Comprehensive configuration validation at startup
//! - **Multi-chain Support**: Configuration for different blockchain networks
//! - **Multi-deployment Support**: Support for different Aave V3 deployments

/// Configuration type definitions and structures
pub mod types;

/// Configuration file loading and environment variable expansion
pub mod load;

/// Configuration validation and integrity checking
pub mod validate;

// Re-export all public items for convenient access
pub use types::*;
pub use load::*;
pub use validate::*;
