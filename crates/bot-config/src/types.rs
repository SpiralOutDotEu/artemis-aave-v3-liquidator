use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use clap::ValueEnum;

/// Main configuration structure for the Artemis liquidator bot
/// 
/// Contains all configuration sections including application settings,
/// blockchain networks, deployments, and execution parameters.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BotConfig {
    /// Application-level configuration (mode, logging, timing)
    pub app: AppCfg,
    /// Blockchain network configurations
    pub chains: BTreeMap<String, ChainCfg>,
    /// Protocol deployment configurations per chain
    pub deployments: BTreeMap<String, BTreeMap<String, DeploymentCfg>>,
    /// Token configurations per chain
    pub tokens: BTreeMap<String, BTreeMap<String, TokenCfg>>,
    /// Strategy-specific configuration parameters
    pub strategy: StrategyCfg,
    /// Execution and transaction parameters
    pub execution: ExecutionCfg,
    /// Rate limiting configuration
    pub rate_limiting: RateLimitingCfg,
    /// Borrower management and reconciliation configuration
    pub borrower_management: BorrowerManagementCfg,
    /// Simulation mode configuration
    pub simulation: SimulationCfg,
}

/// Application-level configuration settings
/// 
/// Controls the bot's operation mode, logging behavior, and timing parameters.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppCfg {
    /// Bot operation mode (live trading or simulation)
    pub mode: Mode,
    /// Logging level for the application
    pub log_level: String,
    /// Directory for storing bot data, cache, and logs
    pub data_dir: String,
    /// Tick interval in milliseconds for time-based collection
    pub tick_ms: u64,
    /// Event collection strategy (time, block, or pending)
    pub collector: CollectorType,
}

/// Bot operation mode enumeration
/// 
/// Determines whether the bot executes real transactions or just simulates them.
#[derive(Clone, Debug, Deserialize, Serialize, ValueEnum)]
pub enum Mode {
    /// Live trading mode - executes real transactions
    #[serde(rename = "live")]
    Live,
    /// Simulation mode - calculates but doesn't execute transactions
    #[serde(rename = "simulate")]
    Simulate,
}

/// Event collection strategy enumeration
/// 
/// Defines how the bot monitors for new events and opportunities.
#[derive(Clone, Debug, Deserialize, Serialize, ValueEnum)]
pub enum CollectorType {
    /// Time-based polling at fixed intervals
    #[serde(rename = "time")]
    Time,
    /// Block-based monitoring for real-time updates
    #[serde(rename = "block")]
    Block,
    /// Pending transaction monitoring
    #[serde(rename = "pending")]
    Pending,
}

/// Configuration for a specific blockchain network
/// 
/// Contains network-specific parameters like RPC endpoints and block ranges.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChainCfg {
    /// Blockchain network identifier (e.g., 8453 for Base)
    pub chain_id: u64,
    /// RPC endpoint URL for blockchain interaction
    pub rpc_url: String,
    /// Blockchain explorer configuration
    pub explorer: ExplorerCfg,
    /// Starting block number for efficient scanning
    pub start_block: u64,
}

/// Blockchain explorer configuration
/// 
/// Provides base URLs for blockchain explorers and related services.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExplorerCfg {
    /// Base URL for the blockchain explorer (e.g., https://basescan.org)
    pub base_url: String,
}

/// Configuration for a specific protocol deployment
/// 
/// Contains contract addresses and parameters for Aave V3 or similar protocols.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeploymentCfg {
    /// Aave V3 Pool contract address
    pub pool: String,
    /// Pool Data Provider contract address
    pub pool_data_provider: String,
    /// Price Oracle contract address
    pub oracle: String,
    /// L2 Encoder contract address (optional)
    pub l2_encoder: Option<String>,
    /// Uniswap V3 integration configuration
    pub uniswap_v3: UniV3Cfg,
    /// Wrapped ETH contract address for the chain
    pub weth: String,
    /// Liquidator contract address (optional)
    pub liquidator: Option<String>,
}

/// Uniswap V3 integration configuration
/// 
/// Contains addresses for Uniswap V3 contracts used in liquidation execution.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UniV3Cfg {
    /// Uniswap V3 Factory contract address
    pub factory: String,
    /// Uniswap V3 Quoter contract address (optional)
    pub quoter: Option<String>,
    /// Uniswap V3 Router contract address (optional)
    pub router: Option<String>,
}

/// Configuration for a specific ERC20 token
/// 
/// Contains token metadata and priority settings for liquidation operations.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TokenCfg {
    /// Token contract address
    pub address: String,
    /// Token decimal places
    pub decimals: u8,
    /// Priority level for liquidation (higher = more preferred)
    pub priority: u8,
}

/// Strategy-specific configuration parameters
/// 
/// Controls the behavior of the liquidation strategy and opportunity selection.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StrategyCfg {
    /// Maximum number of liquidation candidates to consider
    pub max_candidates: u32,
    /// Liquidation close factor in basis points (e.g., 5000 = 50%)
    pub close_factor_bps: u64,
    /// Minimum profit threshold in USD
    pub min_profit_usd: f64,
    /// Slippage tolerance in basis points
    pub slippage_bps: u64,
    /// Whether to prefer aToken positions
    pub prefer_atoken: bool,
}

/// Execution and transaction configuration
/// 
/// Controls how transactions are executed and gas bidding strategies.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExecutionCfg {
    /// List of wallet private keys for transaction signing
    pub wallets: Vec<String>,
    /// List of transaction submitters
    pub submitters: Vec<String>,
    /// Private RPC endpoints for transaction submission
    pub private_endpoints: Vec<String>,
    /// Gas tip as fraction of profit in basis points
    pub tip_fraction_of_profit_bps: u64,
}

/// Simulation mode configuration
/// 
/// Parameters for running the bot in simulation mode without real transactions.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SimulationCfg {
    /// Output directory for simulation results
    pub out_dir: String,
    /// JSONL file for simulation events
    pub jsonl: String,
    /// SQLite database for simulation state
    pub sqlite: String,
    /// Watch for liquidations during simulation
    pub watch_liquidations: bool,
}

/// Rate limiting configuration to avoid RPC throttling
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RateLimitingCfg {
    /// Number of blocks to process per RPC call
    pub log_block_range: u64,
    /// Delay between RPC requests in milliseconds
    pub request_delay_ms: u64,
    /// Maximum concurrent RPC requests
    pub max_concurrent_requests: usize,
    /// Number of retry attempts for failed requests
    pub retry_attempts: u32,
    /// Delay before retrying failed requests in milliseconds
    pub retry_delay_ms: u64,
}

/// Borrower management and reconciliation configuration
/// 
/// Controls how the bot tracks, reconciles, and prunes borrower positions
/// to maintain accurate state and optimal performance.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BorrowerManagementCfg {
    /// TTL for stale borrowers (in blocks) - borrowers older than this will be pruned if they appear to have zero positions
    pub stale_borrower_ttl_blocks: u64,
    /// How often to reconcile dirty borrowers (in blocks) - 0 means every block
    pub reconcile_interval_blocks: u64,
    /// Maximum number of borrowers to reconcile per cycle to avoid RPC overload
    pub max_reconcile_per_cycle: usize,
    /// Whether to enable automatic pruning of zero-position borrowers
    pub enable_auto_pruning: bool,
    /// Whether to track aToken/debt token transfers for more accurate position tracking
    pub track_token_transfers: bool,
    /// Minimum health factor threshold for reconciliation (borrowers below this get priority)
    pub min_hf_threshold_for_reconcile: u64,
    /// Whether to always resume from cache (true) or force fresh start (false)
    pub always_resume_from_cache: bool,
}
