use crate::types::BotConfig;
use anyhow::{anyhow, Result};
use std::env;
use std::path::Path;
use std::fs;
use std::collections::HashMap;

/// Loads bot configuration from a TOML file with environment variable expansion
/// 
/// This function reads the configuration file and automatically expands any
/// environment variable references (e.g., `${BASE_RPC_URL}`) using values from
/// `secrets.env` first, then falling back to shell environment variables.
/// 
/// # Arguments
/// * `path` - Path to the TOML configuration file
/// 
/// # Returns
/// * `Result<BotConfig>` - Parsed configuration or error
/// 
/// # Example
/// ```rust
/// let config = load_from_file("config/bot.toml")?;
/// ```
pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<BotConfig> {
    let path_ref = path.as_ref();
    let content = fs::read_to_string(path_ref)?;
    let mut config: BotConfig = toml::from_str(&content)?;
    
    // Apply environment variable overrides (secrets.env first, then env vars)
    apply_env_overrides(&mut config, path_ref.parent())?;
    
    Ok(config)
}

/// Loads configuration with environment variable support (alias for load_from_file)
/// 
/// This function is provided for backward compatibility and calls `load_from_file`.
/// 
/// # Arguments
/// * `path` - Path to the TOML configuration file
/// 
/// # Returns
/// * `Result<BotConfig>` - Parsed configuration or error
pub fn load_with_env<P: AsRef<Path>>(path: P) -> Result<BotConfig> {
    load_from_file(path)
}

/// Applies environment variable overrides to the configuration
/// 
/// Processes the configuration and replaces any environment variable references
/// (e.g., `${BASE_RPC_URL}`) with actual values. Prioritizes `secrets.env` file
/// over shell environment variables for security.
/// 
/// # Arguments
/// * `config` - Mutable reference to the configuration to modify
/// * `config_dir` - Optional directory containing the configuration file
/// 
/// # Returns
/// * `Result<()>` - Success or error from environment variable expansion
fn apply_env_overrides(config: &mut BotConfig, config_dir: Option<&Path>) -> Result<()> {
    // Load secrets.env first, then fall back to environment variables
    let secrets = load_secrets_env(config_dir);
    
    // Expand environment variables in RPC URLs
    for chain in config.chains.values_mut() {
        if chain.rpc_url.starts_with("${") && chain.rpc_url.ends_with("}") {
            let env_var = &chain.rpc_url[2..chain.rpc_url.len()-1];
            chain.rpc_url = get_env_value(env_var, &secrets)?;
        }
    }

    // Expand environment variables in wallet private keys
    for wallet in config.execution.wallets.iter_mut() {
        if wallet.starts_with("${") && wallet.ends_with("}") {
            let env_var = &wallet[2..wallet.len()-1];
            *wallet = get_env_value(env_var, &secrets)?;
        }
    }

    Ok(())
}

/// Loads secrets from secrets.env file if it exists
/// 
/// Attempts to load environment variables from a `secrets.env` file in the
/// configuration directory. This provides a secure way to manage sensitive
/// configuration values without exposing them in shell environment variables.
/// 
/// # Arguments
/// * `config_dir` - Optional directory containing the configuration file
/// 
/// # Returns
/// * `HashMap<String, String>` - Map of environment variable names to values
fn load_secrets_env(config_dir: Option<&Path>) -> HashMap<String, String> {
    let mut secrets = HashMap::new();
    
    // Try to load from secrets.env in config directory first
    if let Some(dir) = config_dir {
        let secrets_path = dir.join("secrets.env");
        if let Ok(content) = fs::read_to_string(secrets_path) {
            for line in content.lines() {
                let line = line.trim();
                if !line.is_empty() && !line.starts_with('#') {
                    if let Some(pos) = line.find('=') {
                        let key = line[..pos].trim();
                        let value = line[pos + 1..].trim();
                        if !key.is_empty() && !value.is_empty() {
                            secrets.insert(key.to_string(), value.to_string());
                        }
                    }
                }
            }
        }
    }
    
    // Only fall back to config/secrets.env if we're not in a test environment
    // (i.e., if config_dir is None, which means we're loading from the root)
    if secrets.is_empty() && config_dir.is_none() {
        if let Ok(content) = fs::read_to_string("config/secrets.env") {
            for line in content.lines() {
                let line = line.trim();
                if !line.is_empty() && !line.starts_with('#') {
                    if let Some(pos) = line.find('=') {
                        let key = line[..pos].trim();
                        let value = line[pos + 1..].trim();
                        if !key.is_empty() && !value.is_empty() {
                            secrets.insert(key.to_string(), value.to_string());
                        }
                    }
                }
            }
        }
    }
    
    secrets
}

/// Gets environment variable value, trying secrets.env first, then environment
/// 
/// This function implements the priority system for environment variables:
/// 1. First checks the `secrets.env` file (loaded in memory)
/// 2. Falls back to shell environment variables
/// 
/// # Arguments
/// * `key` - Environment variable name to look up
/// * `secrets` - HashMap containing secrets.env values
/// 
/// # Returns
/// * `Result<String>` - Environment variable value or error if not found
fn get_env_value(key: &str, secrets: &HashMap<String, String>) -> Result<String> {
    // First try secrets.env
    if let Some(value) = secrets.get(key) {
        return Ok(value.clone());
    }
    
    // Fall back to environment variable
    env::var(key).map_err(|_| anyhow!("Environment variable {} not found in secrets.env or environment", key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_config_with_env_vars() {
        let config_content = r#"
[app]
mode = "live"
log_level = "info"
data_dir = ".botdata"
tick_ms = 300000
collector = "time"

[chains.base]
chain_id = 8453
rpc_url = "${BASE_RPC_URL}"
explorer.base_url = "https://basescan.org"
start_block = 2963358

[deployments.base.AAVE]
pool = "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5"
pool_data_provider = "0x2d8A3C5677189723C4cB8873CfC9C8976FDF38Ac"
oracle = "0x2Cc0Fc26eD4563A5ce5e8bdcfe1A2878676Ae156"
l2_encoder = "0x39e97c588B2907Fb67F44fea256Ae3BA064207C5"
uniswap_v3.factory = "0x33128a8fC17869897dcE68Ed026d694621f6FDfD"
weth = "0x4200000000000000000000000000000000000006"

[tokens.base.USDC]
address = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
decimals = 6
priority = 10

[tokens.base.WETH]
address = "0x4200000000000000000000000000000000000006"
decimals = 18
priority = 10

[strategy]
max_candidates = 50
close_factor_bps = 5000
min_profit_usd = 10.0
slippage_bps = 50
prefer_atoken = false

[execution]
wallets = ["${PK1}"]
submitters = ["public"]
private_endpoints = []
tip_fraction_of_profit_bps = 500

[simulation]
out_dir = ".botdata/sim"
jsonl = "opportunities.jsonl"
sqlite = "sim.db"
watch_liquidations = true
"#;

        let temp_file = NamedTempFile::new().unwrap();
        std::fs::write(&temp_file, config_content).unwrap();

        // Create a unique temp directory for this test to avoid interference
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        std::fs::write(&config_path, config_content).unwrap();

        // Set environment variables
        env::set_var("BASE_RPC_URL", "https://base-mainnet.example.com");
        env::set_var("PK1", "0x1234567890abcdef");

        let config = load_from_file(&config_path).unwrap();
        
        assert_eq!(config.chains["base"].rpc_url, "https://base-mainnet.example.com");
        assert_eq!(config.execution.wallets[0], "0x1234567890abcdef");
    }

    #[test]
    fn test_load_config_with_secrets_env() {
        let config_content = r#"
[app]
mode = "live"
log_level = "info"
data_dir = ".botdata"
tick_ms = 300000
collector = "time"

[chains.base]
chain_id = 8453
rpc_url = "${BASE_RPC_URL}"
explorer.base_url = "https://basescan.org"
start_block = 2963358

[deployments.base.AAVE]
pool = "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5"
pool_data_provider = "0x2d8A3C5677189723C4cB8873CfC9C8976FDF38Ac"
oracle = "0x2Cc0Fc26eD4563A5ce5e8bdcfe1A2878676Ae156"
l2_encoder = "0x39e97c588B2907Fb67F44fea256Ae3BA064207C5"
uniswap_v3.factory = "0x33128a8fC17869897dcE68Ed026d694621f6FDfD"
weth = "0x4200000000000000000000000000000000000006"

[tokens.base.USDC]
address = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
decimals = 6
priority = 10

[tokens.base.WETH]
address = "0x4200000000000000000000000000000000000006"
decimals = 18
priority = 10

[strategy]
max_candidates = 50
close_factor_bps = 5000
min_profit_usd = 10.0
slippage_bps = 50
prefer_atoken = false

[execution]
wallets = ["${PK1}"]
submitters = ["public"]
private_endpoints = []
tip_fraction_of_profit_bps = 500

[simulation]
out_dir = ".botdata/sim"
jsonl = "opportunities.jsonl"
sqlite = "sim.db"
watch_liquidations = true
"#;

        // Create a unique temp directory for this test to avoid interference
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        std::fs::write(&config_path, config_content).unwrap();

        // Create a temporary secrets.env file in the same directory as the config
        let secrets_path = temp_dir.path().join("secrets.env");
        let secrets_content = "BASE_RPC_URL=https://base-mainnet.secrets.example.com\nPK1=0xabcdef1234567890";
        std::fs::write(&secrets_path, secrets_content).unwrap();



        // Clear any existing environment variables to ensure secrets.env is used
        std::env::remove_var("BASE_RPC_URL");
        std::env::remove_var("PK1");

        let config = load_from_file(&config_path).unwrap();
        
        // Should use secrets.env values
        assert_eq!(config.chains["base"].rpc_url, "https://base-mainnet.secrets.example.com");
        assert_eq!(config.execution.wallets[0], "0xabcdef1234567890");
    }
}
