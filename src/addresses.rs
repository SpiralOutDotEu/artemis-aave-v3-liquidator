use bot_config::{BotConfig, DeploymentCfg};
use ethers::types::Address;
use std::str::FromStr;

/// Centralized address management for blockchain deployments
/// 
/// This struct holds all contract addresses needed for a specific deployment,
/// providing a clean interface for the strategy to access contract locations.
/// Addresses are automatically parsed from configuration strings to ensure
/// proper format validation.
pub struct Addresses {
    /// Aave V3 Pool contract address for lending operations
    pub pool: Address,
    /// Pool Data Provider contract for querying reserve and user data
    pub pool_data_provider: Address,
    /// Price Oracle contract for asset price feeds
    pub oracle: Address,
    /// L2 Encoder contract for batch operations (optional, some deployments don't have this)
    pub l2_encoder: Option<Address>,
    /// Uniswap V3 Factory contract for DEX operations
    pub uniswap_v3_factory: Address,
    /// Uniswap V3 Quoter contract for price estimation (optional)
    pub uniswap_v3_quoter: Option<Address>,
    /// Uniswap V3 Router contract for swap execution (optional)
    pub uniswap_v3_router: Option<Address>,
    /// Wrapped ETH contract address for the target chain
    pub weth: Address,
    /// Liquidator contract address for executing liquidations (optional)
    pub liquidator: Option<Address>,
}

impl From<&DeploymentCfg> for Addresses {
    /// Converts deployment configuration to Addresses struct
    /// 
    /// This implementation automatically parses string addresses from the configuration
    /// into proper Address types, providing compile-time safety and runtime validation.
    /// 
    /// # Arguments
    /// * `deployment` - Reference to deployment configuration containing address strings
    /// 
    /// # Returns
    /// * `Self` - Fully populated Addresses struct with parsed addresses
    fn from(deployment: &DeploymentCfg) -> Self {
        Self {
            pool: Address::from_str(&deployment.pool).unwrap(),
            pool_data_provider: Address::from_str(&deployment.pool_data_provider).unwrap(),
            oracle: Address::from_str(&deployment.oracle).unwrap(),
            l2_encoder: deployment.l2_encoder.as_ref().map(|addr| Address::from_str(addr).unwrap()),
            uniswap_v3_factory: Address::from_str(&deployment.uniswap_v3.factory).unwrap(),
            uniswap_v3_quoter: deployment.uniswap_v3.quoter.as_ref().map(|addr| Address::from_str(addr).unwrap()),
            uniswap_v3_router: deployment.uniswap_v3.router.as_ref().map(|addr| Address::from_str(addr).unwrap()),
            weth: Address::from_str(&deployment.weth).unwrap(),
            liquidator: deployment.liquidator.as_ref().map(|addr| Address::from_str(addr).unwrap()),
        }
    }
}

/// Helper function to retrieve deployment configuration from bot config
/// 
/// This utility function provides a convenient way to access deployment-specific
/// configuration from the main bot configuration, handling the nested structure
/// of chains and deployments.
/// 
/// # Arguments
/// * `config` - Reference to the main bot configuration
/// * `chain` - Chain name identifier (e.g., "base", "arbitrum")
/// * `deployment` - Deployment name identifier (e.g., "AAVE", "SEAMLESS")
/// 
/// # Returns
/// * `Option<&'a DeploymentCfg>` - Reference to deployment config if found, None otherwise
pub fn get_deployment_config<'a>(config: &'a BotConfig, chain: &str, deployment: &str) -> Option<&'a DeploymentCfg> {
    config.deployments.get(chain)?.get(deployment)
}
