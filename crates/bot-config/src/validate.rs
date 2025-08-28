use crate::types::{BotConfig, ChainCfg, DeploymentCfg};
use anyhow::{anyhow, Result};
use std::str::FromStr;

use ethers::types::Address;

/// Validates the complete bot configuration for integrity and correctness
/// 
/// Performs comprehensive validation of all configuration sections including:
/// - Chain configurations (RPC URLs, chain IDs, start blocks)
/// - Deployment configurations (contract addresses, protocol parameters)
/// - Token configurations (addresses, decimals, priorities)
/// - Strategy parameters (limits, thresholds, preferences)
/// - Execution settings (wallets, submitters, gas strategies)
/// 
/// # Arguments
/// * `config` - Reference to the bot configuration to validate
/// 
/// # Returns
/// * `Result<()>` - Success if validation passes, error with details if it fails
/// 
/// # Example
/// ```rust
/// let config = load_from_file("config/bot.toml")?;
/// validate_config(&config)?;
/// ```
pub fn validate_config(config: &BotConfig) -> Result<()> {
    // Validate chains
    for (chain_name, chain) in &config.chains {
        validate_chain(chain_name, chain)?;
    }

    // Validate deployments
    for (chain_name, deployments) in &config.deployments {
        if !config.chains.contains_key(chain_name) {
            return Err(anyhow!("Deployment chain '{}' not found in chains", chain_name));
        }
        
        for (deployment_name, deployment) in deployments {
            validate_deployment(chain_name, deployment_name, deployment)?;
        }
    }

    // Validate tokens
    for (chain_name, tokens) in &config.tokens {
        if !config.chains.contains_key(chain_name) {
            return Err(anyhow!("Token chain '{}' not found in chains", chain_name));
        }
        
        for (token_name, token) in tokens {
            validate_token(chain_name, token_name, token)?;
        }
    }

    // Validate strategy
    validate_strategy(&config.strategy)?;

    // Validate execution
    validate_execution(&config.execution)?;

    Ok(())
}

/// Validates a single chain configuration
/// 
/// Checks that the chain has valid parameters including:
/// - Non-zero chain ID
/// - Non-empty RPC URL
/// - Valid start block number
/// 
/// # Arguments
/// * `chain_name` - Name of the chain for error reporting
/// * `chain` - Chain configuration to validate
/// 
/// # Returns
/// * `Result<()>` - Success if validation passes, error if it fails
fn validate_chain(chain_name: &str, chain: &ChainCfg) -> Result<()> {
    if chain.chain_id == 0 {
        return Err(anyhow!("Chain '{}' has invalid chain_id", chain_name));
    }
    
    if chain.rpc_url.is_empty() {
        return Err(anyhow!("Chain '{}' has empty RPC URL", chain_name));
    }
    
    if chain.start_block == 0 {
        return Err(anyhow!("Chain '{}' has invalid start_block", chain_name));
    }
    
    Ok(())
}

/// Validates a single deployment configuration
/// 
/// Performs comprehensive validation of deployment parameters including:
/// - Valid Ethereum addresses for all contracts
/// - Proper format for pool, oracle, and data provider addresses
/// - Optional addresses (L2 encoder, liquidator) when provided
/// - Uniswap V3 integration addresses
/// 
/// # Arguments
/// * `chain_name` - Name of the chain for error reporting
/// * `deployment_name` - Name of the deployment for error reporting
/// * `deployment` - Deployment configuration to validate
/// 
/// # Returns
/// * `Result<()>` - Success if validation passes, error if it fails
fn validate_deployment(chain_name: &str, deployment_name: &str, deployment: &DeploymentCfg) -> Result<()> {
    // Validate addresses are valid hex
    if let Err(_) = Address::from_str(&deployment.pool) {
        return Err(anyhow!("Invalid pool address in deployment {}.{}", chain_name, deployment_name));
    }
    
    if let Err(_) = Address::from_str(&deployment.pool_data_provider) {
        return Err(anyhow!("Invalid pool_data_provider address in deployment {}.{}", chain_name, deployment_name));
    }
    
    if let Err(_) = Address::from_str(&deployment.oracle) {
        return Err(anyhow!("Invalid oracle address in deployment {}.{}", chain_name, deployment_name));
    }
    
    if let Some(l2_encoder) = &deployment.l2_encoder {
        if let Err(_) = Address::from_str(l2_encoder) {
            return Err(anyhow!("Invalid l2_encoder address in deployment {}.{}", chain_name, deployment_name));
        }
    }
    
    if let Err(_) = Address::from_str(&deployment.weth) {
        return Err(anyhow!("Invalid WETH address in deployment {}.{}", chain_name, deployment_name));
    }
    
    if let Some(liquidator) = &deployment.liquidator {
        if let Err(_) = Address::from_str(liquidator) {
            return Err(anyhow!("Invalid liquidator address in deployment {}.{}", chain_name, deployment_name));
        }
    }
    
    // Validate Uniswap V3 addresses
    if let Err(_) = Address::from_str(&deployment.uniswap_v3.factory) {
        return Err(anyhow!("Invalid Uniswap V3 factory address in deployment {}.{}", chain_name, deployment_name));
    }
    
    if let Some(quoter) = &deployment.uniswap_v3.quoter {
        if let Err(_) = Address::from_str(quoter) {
            return Err(anyhow!("Invalid Uniswap V3 quoter address in deployment {}.{}", chain_name, deployment_name));
        }
    }
    
    if let Some(router) = &deployment.uniswap_v3.router {
        if let Err(_) = Address::from_str(router) {
            return Err(anyhow!("Invalid Uniswap V3 router address in deployment {}.{}", chain_name, deployment_name));
        }
    }
    
    Ok(())
}

/// Validates a single token configuration
/// 
/// Checks that the token has valid parameters including:
/// - Valid Ethereum address format
/// - Reasonable decimal places (0-18)
/// 
/// # Arguments
/// * `chain_name` - Name of the chain for error reporting
/// * `token_name` - Name of the token for error reporting
/// * `token` - Token configuration to validate
/// 
/// # Returns
/// * `Result<()>` - Success if validation passes, error if it fails
fn validate_token(chain_name: &str, token_name: &str, token: &crate::types::TokenCfg) -> Result<()> {
    if let Err(_) = Address::from_str(&token.address) {
        return Err(anyhow!("Invalid token address for {}.{}", chain_name, token_name));
    }
    
    if token.decimals > 18 {
        return Err(anyhow!("Token {}.{} has invalid decimals: {}", chain_name, token_name, token.decimals));
    }
    
    Ok(())
}

/// Validates strategy configuration parameters
/// 
/// Ensures strategy parameters are within reasonable bounds:
/// - Maximum candidates must be positive
/// - Close factor must not exceed 100%
/// - Minimum profit must be positive
/// - Slippage tolerance must not exceed 10%
/// 
/// # Arguments
/// * `strategy` - Strategy configuration to validate
/// 
/// # Returns
/// * `Result<()>` - Success if validation passes, error if it fails
fn validate_strategy(strategy: &crate::types::StrategyCfg) -> Result<()> {
    if strategy.max_candidates == 0 {
        return Err(anyhow!("max_candidates must be greater than 0"));
    }
    
    if strategy.close_factor_bps > 10000 {
        return Err(anyhow!("close_factor_bps cannot exceed 10000 (100%)"));
    }
    
    if strategy.min_profit_usd <= 0.0 {
        return Err(anyhow!("min_profit_usd must be positive"));
    }
    
    if strategy.slippage_bps > 1000 {
        return Err(anyhow!("slippage_bps cannot exceed 1000 (10%)"));
    }
    
    Ok(())
}

/// Validates execution configuration parameters
/// 
/// Ensures execution settings are properly configured:
/// - At least one wallet must be provided
/// - At least one submitter must be configured
/// - Gas tip fraction must not exceed 10%
/// 
/// # Arguments
/// * `execution` - Execution configuration to validate
/// 
/// # Returns
/// * `Result<()>` - Success if validation passes, error if it fails
fn validate_execution(execution: &crate::types::ExecutionCfg) -> Result<()> {
    if execution.wallets.is_empty() {
        return Err(anyhow!("At least one wallet must be configured"));
    }
    
    if execution.submitters.is_empty() {
        return Err(anyhow!("At least one submitter must be configured"));
    }
    
    if execution.tip_fraction_of_profit_bps > 1000 {
        return Err(anyhow!("tip_fraction_of_profit_bps cannot exceed 1000 (10%)"));
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    #[test]
    fn test_validate_valid_config() {
        let config = BotConfig {
            app: AppCfg {
                mode: Mode::Live,
                log_level: "info".to_string(),
                data_dir: ".botdata".to_string(),
                tick_ms: 300000,
                collector: CollectorType::Time,
            },
            chains: {
                let mut map = BTreeMap::new();
                map.insert("base".to_string(), ChainCfg {
                    chain_id: 8453,
                    rpc_url: "https://base.example.com".to_string(),
                    explorer: ExplorerCfg { base_url: "https://basescan.org".to_string() },
                    start_block: 2963358,
                });
                map
            },
            deployments: {
                let mut map = BTreeMap::new();
                let mut deployments = BTreeMap::new();
                deployments.insert("AAVE".to_string(), DeploymentCfg {
                    pool: "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5".to_string(),
                    pool_data_provider: "0x2d8A3C5677189723C4cB8873CfC9C8976FDF38Ac".to_string(),
                    oracle: "0x2Cc0Fc26eD4563A5ce5e8bdcfe1A2878676Ae156".to_string(),
                    l2_encoder: Some("0x39e97c588B2907Fb67F44fea256Ae3BA064207C5".to_string()),
                    uniswap_v3: UniV3Cfg {
                        factory: "0x33128a8fC17869897dcE68Ed026d694621f6FDfD".to_string(),
                        quoter: None,
                        router: None,
                    },
                    weth: "0x4200000000000000000000000000000000000006".to_string(),
                    liquidator: None,
                });
                map.insert("base".to_string(), deployments);
                map
            },
            tokens: {
                let mut map = BTreeMap::new();
                let mut tokens = BTreeMap::new();
                tokens.insert("WETH".to_string(), TokenCfg {
                    address: "0x4200000000000000000000000000000000000006".to_string(),
                    decimals: 18,
                    priority: 10,
                });
                map.insert("base".to_string(), tokens);
                map
            },
            strategy: StrategyCfg {
                max_candidates: 50,
                close_factor_bps: 5000,
                min_profit_usd: 10.0,
                slippage_bps: 50,
                prefer_atoken: false,
            },
            execution: ExecutionCfg {
                wallets: vec!["0x1234567890abcdef".to_string()],
                submitters: vec!["public".to_string()],
                private_endpoints: vec![],
                tip_fraction_of_profit_bps: 500,
            },
            simulation: SimulationCfg {
                out_dir: ".botdata/sim".to_string(),
                jsonl: "opportunities.jsonl".to_string(),
                sqlite: "sim.db".to_string(),
                watch_liquidations: true,
            },
        };
        
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_invalid_chain_id() {
        let config = BotConfig {
            app: AppCfg {
                mode: Mode::Live,
                log_level: "info".to_string(),
                data_dir: ".botdata".to_string(),
                tick_ms: 300000,
                collector: CollectorType::Time,
            },
            chains: {
                let mut map = BTreeMap::new();
                map.insert("base".to_string(), ChainCfg {
                    chain_id: 0, // Invalid
                    rpc_url: "https://base.example.com".to_string(),
                    explorer: ExplorerCfg { base_url: "https://basescan.org".to_string() },
                    start_block: 2963358,
                });
                map
            },
            deployments: BTreeMap::new(),
            tokens: BTreeMap::new(),
            strategy: StrategyCfg {
                max_candidates: 50,
                close_factor_bps: 5000,
                min_profit_usd: 10.0,
                slippage_bps: 50,
                prefer_atoken: false,
            },
            execution: ExecutionCfg {
                wallets: vec!["0x1234567890abcdef".to_string()],
                submitters: vec!["public".to_string()],
                private_endpoints: vec![],
                tip_fraction_of_profit_bps: 500,
            },
            simulation: SimulationCfg {
                out_dir: ".botdata/sim".to_string(),
                jsonl: "opportunities.jsonl".to_string(),
                sqlite: "sim.db".to_string(),
                watch_liquidations: true,
            },
        };
        
        assert!(validate_config(&config).is_err());
    }
}
