use crate::collectors::time_collector::NewTick;
use crate::storage::Storage;
use bot_config::types::BorrowerManagementCfg;

use crate::strategies::types::Config;
use anyhow::{anyhow, Result};
use artemis_core::executors::mempool_executor::{GasBidInfo, SubmitTxToMempool};
use artemis_core::types::Strategy;
use async_trait::async_trait;
use bindings_aave::{
    i_aave_oracle::IAaveOracle,
    i_pool_data_provider::IPoolDataProvider,
    ierc20::IERC20,
    l2_encoder::L2Encoder,
    pool::{BorrowFilter, Pool, SupplyFilter, RepayFilter, WithdrawFilter, LiquidationCallFilter},
};
use bindings_liquidator::liquidator::Liquidator;
use clap::{Parser, ValueEnum};
use ethers::{
    contract::builders::ContractCall,
    providers::Middleware,
    types::{transaction::eip2718::TypedTransaction, Address, ValueOrArray, H160, I256, U256, U64},
};
use ethers_contract::Multicall;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::iter::zip;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{error, info, warn};
use tokio::time;

use super::types::{Action, Event};



/// Configuration for a specific blockchain deployment
/// 
/// Contains all contract addresses and blockchain-specific parameters
/// needed to interact with a particular Aave V3 deployment.
#[derive(Debug)]
struct DeploymentConfig {
    /// Aave V3 Pool contract address for lending operations
    pool_address: Address,
    /// Pool Data Provider contract for querying reserve and user data
    pool_data_provider: Address,
    /// Price Oracle contract for asset price feeds
    oracle_address: Address,
    /// L2 Encoder contract for batch operations (optional)
    l2_encoder: Address,
    /// Block number when the deployment was created (for efficient scanning)
    creation_block: u64,
}

/// Supported deployment types for the liquidator bot
/// 
/// Each deployment represents a different Aave V3 instance or fork
/// with its own set of contract addresses and parameters.
#[derive(Debug, Clone, Parser, ValueEnum)]
pub enum Deployment {
    /// Standard Aave V3 deployment
    AAVE,
    /// Seamless Protocol deployment (Aave V3 fork)
    SEAMLESS,
}

// Configuration constants that were previously hardcoded
// These are now configurable via bot.toml for flexibility
// pub const WETH_ADDRESS: &str = "0x4200000000000000000000000000000000000006";
// pub const LIQUIDATION_CLOSE_FACTOR_THRESHOLD: &str = "950000000000000000";
// pub const MAX_LIQUIDATION_CLOSE_FACTOR: u64 = 10000;
// pub const DEFAULT_LIQUIDATION_CLOSE_FACTOR: u64 = 5000;

/// Operational constants for the liquidator bot
/// 
/// These values are tuned for optimal performance and RPC compatibility
pub const DEFAULT_LOG_BLOCK_RANGE: u64 = 100; // Default block range for log queries
pub const DEFAULT_MULTICALL_CHUNK_SIZE: usize = 50; // Default batch size for multicall operations
pub const PRICE_ONE: u64 = 100000000; // Price precision constant (8 decimals)

/// Persistent state cache for bot restarts
/// 
/// Stores the last processed block number and borrower information
/// to enable efficient resumption of operations after restarts.
#[derive(Debug, Serialize, Deserialize)]
pub struct StateCache {
    /// Last blockchain block that was processed
    pub last_block_number: u64,
    /// Mapping of borrower addresses to their current state
    pub borrowers: HashMap<Address, Borrower>,
}

/// Current state of the Aave V3 pool
/// 
/// Contains real-time information about asset prices and pool conditions
/// that is used for liquidation calculations and opportunity assessment.
struct PoolState {
    /// Current USD prices for all supported assets
    prices: HashMap<Address, U256>,
}

/// Represents a user's position in the Aave V3 protocol
/// 
/// Tracks both collateral assets and debt positions for a single user,
/// enabling efficient health factor calculations and liquidation detection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Borrower {
    /// User's wallet address
    pub address: Address,
    /// Set of assets the user has supplied as collateral
    pub collateral: HashSet<Address>,
    /// Set of assets the user has borrowed (creating debt)
    pub debt: HashSet<Address>,
    /// Whether this borrower needs reconciliation from chain (marked as dirty)
    pub is_dirty: bool,
    /// Last block number where this borrower's position was modified
    pub last_touched_block: u64,
    /// Last known health factor (in basis points, 1000 = 1.0)
    pub last_health_factor: Option<u64>,
    /// Last time this borrower was reconciled from chain
    pub last_reconciled_block: Option<u64>,
}

/// Configuration for a specific ERC20 token in the Aave V3 protocol
/// 
/// Contains all parameters needed to interact with a token, including
/// risk parameters and protocol-specific settings.
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenConfig {
    /// Token contract address
    address: Address,
    /// Aave V3 aToken address for this asset
    a_address: Address,
    /// Token decimal places
    decimals: u64,
    /// Loan-to-Value ratio (collateralization requirement)
    ltv: u64,
    /// Liquidation threshold (when liquidation becomes possible)
    liquidation_threshold: u64,
    /// Liquidation bonus (incentive for liquidators)
    liquidation_bonus: u64,
    /// Protocol fee percentage
    reserve_factor: u64,
    /// Additional protocol fee
    protocol_fee: u64,
}

/// Main strategy implementation for Aave V3 liquidation
/// 
/// This strategy monitors the Aave V3 protocol for users whose positions
/// have become unhealthy (health factor < 1.0) and executes liquidations
/// when profitable opportunities are detected.
#[allow(dead_code)]
pub struct AaveStrategy<M> {
    /// Ethers client for blockchain interaction
    client: Arc<M>,
    /// Percentage of profits to bid in gas (in basis points)
    bid_percentage: u64,
    /// Last blockchain block that was processed
    last_block_number: u64,
    /// Current state of all monitored borrowers
    borrowers: HashMap<Address, Borrower>,
    /// Configuration for all supported tokens
    tokens: HashMap<Address, TokenConfig>,
    /// Heartbeat counter for progress indicators
    heartbeat_counter: u64,
    /// Blockchain network identifier
    chain_id: u64,
    /// Deployment-specific configuration and addresses
    config: DeploymentConfig,
    /// Liquidator contract address for executing liquidations
    liquidator: Address,
    /// Wrapped ETH contract address for the target chain
    weth_address: Address,
    /// Health factor threshold for determining close factor (in wei)
    close_factor_threshold: U256,
    /// Maximum liquidation close factor (100% = 10000 basis points)
    max_close_factor: U256,
    /// Default liquidation close factor (50% = 5000 basis points)
    default_close_factor: U256,
    /// Data directory for storing cache and logs
    data_dir: String,
    /// Storage backend for borrower state persistence
    storage: Arc<dyn Storage>,
    /// Rate limiting configuration
    log_block_range: u64,
    request_delay_ms: u64,
    max_concurrent_requests: usize,
    retry_attempts: u32,
    retry_delay_ms: u64,
    /// Borrower management configuration
    borrower_config: BorrowerManagementCfg,
}

impl<M: Middleware + 'static> AaveStrategy<M> {


    /// Creates a new AaveStrategy instance
    /// 
    /// Initializes the strategy with the provided configuration and sets up
    /// all necessary parameters for monitoring and liquidating positions.
    /// 
    /// # Arguments
    /// * `client` - Ethers client for blockchain interaction
    /// * `config` - Strategy configuration parameters
    /// * `_deployment` - Target deployment type (currently unused)
    /// * `addresses` - Contract addresses for the target deployment
    /// * `start_block` - Starting block number for efficient scanning
    /// * `data_dir` - Directory for storing persistent data
    /// * `storage` - Storage backend for state persistence
    /// 
    /// # Returns
    /// * `Self` - Fully initialized AaveStrategy instance
    pub fn new(
        client: Arc<M>,
        config: Config,
        _deployment: Deployment,
        addresses: crate::addresses::Addresses,
        start_block: u64,
        data_dir: String,
        storage: Arc<dyn Storage>,
        borrower_config: Option<BorrowerManagementCfg>,
    ) -> Self {
        let borrower_config = borrower_config.unwrap_or_else(|| BorrowerManagementCfg {
            stale_borrower_ttl_blocks: 10000,
            reconcile_interval_blocks: 100,
            max_reconcile_per_cycle: 50,
            enable_auto_pruning: true,
            track_token_transfers: true,
            min_hf_threshold_for_reconcile: 1000,
            always_resume_from_cache: true,
        });
        
        Self {
            client,
            bid_percentage: config.bid_percentage,
            last_block_number: start_block, // Use provided start block for efficient scanning
            borrowers: HashMap::new(),
            tokens: HashMap::new(),
            heartbeat_counter: 0,
            chain_id: config.chain_id,
            config: DeploymentConfig {
                pool_address: addresses.pool,
                pool_data_provider: addresses.pool_data_provider,
                oracle_address: addresses.oracle,
                l2_encoder: addresses.l2_encoder.unwrap_or_else(|| Address::zero()),
                creation_block: start_block, // Use start block as creation block for scanning
            },
            liquidator: addresses.liquidator.unwrap_or_else(|| Address::zero()),
            weth_address: addresses.weth,
            close_factor_threshold: U256::from_str("950000000000000000").unwrap(),
            max_close_factor: U256::from(10000),
            default_close_factor: U256::from(5000),
            data_dir,
            storage,
            // Rate limiting configuration with defaults
            log_block_range: 100,
            request_delay_ms: 500,
            max_concurrent_requests: 3,
            retry_attempts: 3,
            retry_delay_ms: 1000,
            // Borrower management configuration
            borrower_config,
        }
    }
}

/// Represents a profitable liquidation opportunity
/// 
/// Contains all the information needed to execute a liquidation,
/// including the target user, assets involved, and expected profit.
struct LiquidationOpportunity {
    /// Address of the user to be liquidated
    borrower: Address,
    /// Asset being liquidated as collateral
    collateral: Address,
    /// Asset being repaid (debt)
    debt: Address,
    /// Amount of debt to be covered in the liquidation
    debt_to_cover: U256,
    /// Expected profit in ETH terms
    profit_eth: I256,
}

/// Detailed information about a liquidation event that occurred
/// 
/// Captures all available information about liquidations to analyze
/// why our bot missed the opportunity and improve future performance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissedLiquidationEvent {
    /// Unique identifier for this liquidation event
    pub id: String,
    /// Transaction hash of the liquidation
    pub tx_hash: H160,
    /// Block number where liquidation occurred
    pub block_number: u64,
    /// Block timestamp when liquidation occurred
    pub block_timestamp: u64,
    /// Address of the collateral asset being liquidated
    pub collateral_asset: Address,
    /// Address of the debt asset being repaid
    pub debt_asset: Address,
    /// Address of the user being liquidated
    pub user: Address,
    /// Amount of debt covered in the liquidation
    pub debt_to_cover: U256,
    /// Amount of collateral liquidated
    pub liquidated_collateral_amount: U256,
    /// Address of the liquidator who executed the liquidation
    pub liquidator: Address,
    /// Whether the liquidator received aTokens
    pub receive_a_token: bool,
    /// Log index within the block
    pub log_index: u64,
    /// Gas price used for the transaction
    pub gas_price: Option<U256>,
    /// Gas used for the transaction
    pub gas_used: Option<U256>,
    /// Transaction fee in ETH
    pub tx_fee: Option<U256>,
    /// Block base fee (EIP-1559)
    pub base_fee: Option<U256>,
    /// Priority fee (EIP-1559)
    pub priority_fee: Option<U256>,
    /// Whether this was a missed opportunity (we were tracking the borrower)
    pub was_tracked_borrower: bool,
    /// Our bot's health factor for this user at the time (if available)
    pub our_health_factor: Option<U256>,
    /// Estimated profit we would have made (if calculable)
    pub estimated_profit: Option<I256>,
    /// Reason why we missed this opportunity (if determinable)
    pub missed_reason: Option<String>,
    /// When this event was recorded in our database
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

/// Transaction details for analyzing missed liquidations
#[derive(Debug, Clone)]
pub struct TransactionDetails {
    /// Gas price used for the transaction
    pub gas_price: Option<U256>,
    /// Gas used for the transaction
    pub gas_used: Option<U256>,
    /// Total transaction fee
    pub fee: Option<U256>,
    /// Maximum priority fee per gas (EIP-1559)
    pub max_priority_fee_per_gas: Option<U256>,
}

/// Block details for analyzing missed liquidations
#[derive(Debug, Clone)]
pub struct BlockDetails {
    /// Block timestamp
    pub timestamp: Option<u64>,
    /// Base fee per gas (EIP-1559)
    pub base_fee_per_gas: Option<U256>,
}

#[async_trait]
impl<M: Middleware + 'static> Strategy<Event, Action> for AaveStrategy<M> {
    /// Synchronizes the strategy's internal state with the blockchain
    /// 
    /// This method is called during startup to ensure the strategy has
    /// the latest information about tokens, approvals, and borrower positions.
    /// It intelligently resumes from the last processed block to avoid reprocessing
    /// historical data unnecessarily.
    /// 
    /// # Returns
    /// * `Result<()>` - Success or error from synchronization
    async fn sync_state(&mut self) -> Result<()> {
        info!("🔄 Starting state synchronization...");
        info!("📍 Configured start block: {} (from bot.toml)", self.config.creation_block);

        self.update_token_configs().await?;
        self.approve_tokens().await?;
        self.load_cache().await?;
        
        info!("📍 Resume block: {} (will process from here)", self.last_block_number);
        
        // Only update state if we have blocks to process
        let latest_block = self.client.get_block_number().await?;
        if latest_block.as_u64() > self.last_block_number {
            info!("📊 Processing {} blocks to catch up to current blockchain state", 
                  latest_block.as_u64() - self.last_block_number);
            self.update_state().await?;
        } else {
            info!("✅ Bot is already up to date at block {}", latest_block);
        }

        info!("✅ State synchronization complete");
        
        // Show final state summary
        self.show_state_summary();
        
        Ok(())
    }

    /// Processes incoming events and determines appropriate actions
    /// 
    /// This is the main event processing loop that handles different types
    /// of events and converts them into executable actions for the bot.
    /// 
    /// # Arguments
    /// * `event` - The event to process
    /// 
    /// # Returns
    /// * `Vec<Action>` - List of actions to execute (can be empty)
    async fn process_event(&mut self, event: Event) -> Vec<Action> {
        match event {
            // TODO: Implement block-based event processing in Milestone 4
            // Event::NewBlock(block) => self.process_new_block_event(block).await,
            Event::NewTick(block) => {
                if let Some(action) = self.process_new_tick_event(block).await {
                    vec![action]
                } else {
                    vec![]
                }
            }
        }
    }
}

impl<M: Middleware + 'static> AaveStrategy<M> {
    /// Processes new block events (currently unused)
    /// 
    /// This method will be implemented in Milestone 4 to provide
    /// real-time block-based monitoring instead of time-based polling.
    /// 
    /// # Arguments
    /// * `event` - New block event information
    /// 
    /// # Returns
    /// * `Option<Action>` - Action to execute, if any
    // async fn process_new_block_event(&mut self, event: NewBlock) -> Option<Action> {
    //     info!("received new block: {:?}", event);
    //     self.last_block_number = event.number.as_u64();
    //     None
    // }

    /// Processes time-based tick events and updates internal state
    /// 
    /// This is the main processing loop that runs on each tick interval.
    /// It updates the bot's state, scans for opportunities, and executes
    /// liquidations when profitable.
    /// 
    /// # Arguments
    /// * `event` - Time tick event with timestamp information
    /// 
    /// # Returns
    /// * `Option<Action>` - Liquidation transaction to submit, if profitable
    async fn process_new_tick_event(&mut self, event: NewTick) -> Option<Action> {
        self.heartbeat_counter += 1;
        
        // Show heartbeat every few ticks to prove we're alive
        if self.heartbeat_counter % 5 == 0 {
            info!("💓 Bot heartbeat #{} - Alive and monitoring at timestamp: {}", 
                  self.heartbeat_counter, event.timestamp);
        }
        
        info!("🔄 Processing tick #{} at timestamp: {} (UTC: {})", 
              self.heartbeat_counter, event.timestamp, 
              chrono::DateTime::from_timestamp(event.timestamp as i64, 0)
                  .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                  .unwrap_or_else(|| "unknown".to_string()));
        
        // Update state and show progress
        match self.update_state().await {
            Ok(_) => info!("✅ State updated successfully"),
            Err(e) => {
                error!("❌ Update State error: {}", e);
                return None;
            }
        }

        info!("📊 Total borrowers monitored: {}", self.borrowers.len());
        info!("📍 Current blockchain position: block {} (last processed: {})", 
              self.client.get_block_number().await.unwrap_or_default(), self.last_block_number);
        
        // Check for liquidation opportunities
        info!("🔍 Scanning for liquidation opportunities...");
        let op = match self.get_best_liquidation_op().await {
            Ok(Some(op)) => {
                info!("💰 Found opportunity! Profit: {} ETH", op.profit_eth);
                op
            }
            Ok(None) => {
                info!("💤 No liquidation opportunities found at this time");
                return None;
            }
            Err(e) => {
                error!("❌ Error finding liquidation ops: {}", e);
                return None;
            }
        };

        if op.profit_eth < I256::from(0) {
            info!("⚠️ Opportunity not profitable, skipping");
            return None;
        }

        info!("🚀 Executing liquidation for profit: {} ETH", op.profit_eth);
        return Some(Action::SubmitTx(SubmitTxToMempool {
            tx: self
                .build_liquidation(&op)
                .await
                .map_err(|e| error!("Error building liquidation: {}", e))
                .ok()?,
            gas_bid_info: Some(GasBidInfo {
                bid_percentage: self.bid_percentage,
                total_profit: U256::from_dec_str(&op.profit_eth.to_string())
                    .map_err(|e| error!("Failed to bid: {}", e))
                    .ok()?,
            }),
        }));
    }

    /// Identifies all borrowers with health factor below 1.0 (underwater positions)
    /// 
    /// Uses multicall to efficiently query health factors for all known borrowers
    /// and returns them sorted by health factor (most unhealthy first).
    /// 
    /// # Returns
    /// * `Result<Vec<(Address, U256)>>` - List of underwater borrowers with their health factors
    async fn get_underwater_borrowers(&mut self) -> Result<Vec<(Address, U256)>> {
        let pool = Pool::<M>::new(self.config.pool_address, self.client.clone());

        let mut underwater_borrowers = Vec::new();

        // Use multicall to efficiently query multiple users' account data
        let mut multicall = Multicall::new(
            self.client.clone(),
            Some(H160::from_str(
                "0xcA11bde05977b3631167028862bE2a173976CA11",
            )?),
        )
        .await?;
        
        // Filter borrowers to only those with active debt
        let borrowers: Vec<&Borrower> = self
            .borrowers
            .values()
            .filter(|b| b.debt.len() > 0)
            .collect();

        // Process borrowers in chunks to avoid RPC limits
        for chunk in borrowers.chunks(self.max_concurrent_requests) {
            multicall.clear_calls();

            for borrower in chunk {
                multicall.add_call(pool.get_user_account_data(borrower.address), false);
            }

            let result: Vec<(U256, U256, U256, U256, U256, U256)> = multicall.call_array().await?;
            for (borrower, (_, _, _, _, _, health_factor)) in zip(chunk, result) {
                if health_factor.lt(&U256::from_dec_str("1000000000000000000").unwrap()) {
                    info!(
                        "Found underwater borrower {:?} -  healthFactor: {}",
                        borrower, health_factor
                    );
                    underwater_borrowers.push((borrower.address, health_factor));
                }
            }
        }

        // Sort borrowers by health factor (most unhealthy first)
        underwater_borrowers.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(underwater_borrowers)
    }

    /// Loads borrower state from persistent storage
    /// 
    /// Attempts to restore the bot's state from a previous run, including
    /// the last processed block number and borrower information. If no cache
    /// exists, initializes with the deployment's creation block.
    /// 
    /// # Returns
    /// * `Result<()>` - Success or error from cache loading
    async fn load_cache(&mut self) -> Result<()> {
        match self.storage.load_cache().await {
            Ok(cache) => {
                info!("📂 Loaded state cache from storage: {} borrowers, last processed block: {}", 
                      cache.borrowers.len(), cache.last_block_number);
                
                // CRITICAL: Never resume from a block earlier than the configured start_block
                if cache.last_block_number < self.config.creation_block {
                    warn!("⚠️ Cached block {} is earlier than configured start_block {}. Ignoring cache and starting fresh.", 
                          cache.last_block_number, self.config.creation_block);
                    self.last_block_number = self.config.creation_block;
                    self.borrowers = HashMap::new();
                    info!("🆕 Bot will start fresh from configured start block: {} (ignoring invalid cache)", self.config.creation_block);
                } else if self.borrower_config.always_resume_from_cache {
                    // Use the cached last block as our starting point for this run
                    // This ensures we resume from where we left off, not from the beginning
                    self.last_block_number = cache.last_block_number;
                    self.borrowers = cache.borrowers;
                    
                    info!("🔄 Bot will resume processing from block {} (cached from previous run)", self.last_block_number);
                } else {
                    // Force fresh start from configured start block
                    info!("🔄 Force fresh start: ignoring cache, starting from configured start block: {}", self.config.creation_block);
                    self.last_block_number = self.config.creation_block;
                    self.borrowers = HashMap::new();
                }
            }
            Err(e) => {
                info!("📂 No existing cache found: {}, starting fresh from configured start block: {}", e, self.config.creation_block);
                self.last_block_number = self.config.creation_block;
                info!("🆕 Bot will start fresh from block {} (configured in bot.toml)", self.last_block_number);
            }
        };

        Ok(())
    }

    /// Updates the bot's internal state by processing all blockchain events since last update
    /// 
    /// This comprehensive method processes multiple types of events to maintain an accurate
    /// picture of all borrower positions. It intelligently resumes from the last processed
    /// block, ensuring no blocks are missed between restarts.
    /// 
    /// The method processes events in chunks to respect RPC limits and provides
    /// detailed progress information for monitoring.
    /// 
    /// # Returns
    /// * `Result<()>` - Success or error from state update
    async fn update_state(&mut self) -> Result<()> {
        let latest_block = self.client.get_block_number().await?;
        let total_blocks = latest_block.as_u64() - self.last_block_number;
        
        // Safety check: ensure we never go backwards
        if latest_block.as_u64() < self.last_block_number {
            warn!("⚠️ Blockchain appears to have reverted: current block {} < last processed block {}", 
                  latest_block, self.last_block_number);
            warn!("🔄 Adjusting last_block_number to current block to prevent errors");
            self.last_block_number = latest_block.as_u64();
            return Ok(());
        }
        
        // Show resume information
        if total_blocks == 0 {
            info!("💤 No new blocks to process - bot is up to date at block {}", latest_block);
            return Ok(());
        }
        
        // Check if this is a fresh start or a resume
        if self.last_block_number == self.config.creation_block {
            info!("🆕 Fresh start: Processing {} blocks from configured start block {} to current block {}", 
                  total_blocks, self.config.creation_block, latest_block);
        } else {
            info!("🔄 Resuming: Processing {} blocks from last processed block {} to current block {} (gap: {} blocks)", 
                  total_blocks, self.last_block_number, latest_block, total_blocks);
        }

        if total_blocks > 0 {
            // Process all relevant events to maintain accurate borrower state
            let mut borrowers_to_remove = Vec::new();
            
            // 1. Process borrow events (add debt)
            info!("📥 Fetching borrow logs for {} blocks...", total_blocks);
            let borrow_logs = self.get_borrow_logs(self.last_block_number.into(), latest_block).await?;
            info!("📊 Found {} borrow logs, processing borrowers...", borrow_logs.len());
            
            let mut new_borrowers = 0;
            let mut existing_borrowers = 0;
            
            borrow_logs.into_iter().for_each(|log| {
                let user = log.on_behalf_of;
                if self.borrowers.contains_key(&user) {
                    let borrower = self.borrowers.get_mut(&user).unwrap();
                    borrower.debt.insert(log.reserve);
                    borrower.is_dirty = true;
                    borrower.last_touched_block = latest_block.as_u64();
                    existing_borrowers += 1;
                } else {
                    self.borrowers.insert(
                        user,
                        Borrower {
                            address: user,
                            collateral: HashSet::new(),
                            debt: HashSet::from([log.reserve]),
                            is_dirty: true,
                            last_touched_block: latest_block.as_u64(),
                            last_health_factor: None,
                            last_reconciled_block: None,
                        },
                    );
                    new_borrowers += 1;
                }
            });
            
            info!("📈 Borrow logs processed: {} new borrowers, {} existing borrowers updated", 
                  new_borrowers, existing_borrowers);

            // 2. Process supply events (add collateral)
            info!("📥 Fetching supply logs for {} blocks...", total_blocks);
            let supply_logs = self.get_supply_logs(self.last_block_number.into(), latest_block).await?;
            info!("📊 Found {} supply logs, processing collateral...", supply_logs.len());
            
            let mut new_collateral_users = 0;
            let mut existing_collateral_users = 0;
            
            supply_logs.into_iter().for_each(|log| {
                let user = log.on_behalf_of;
                if self.borrowers.contains_key(&user) {
                    let borrower = self.borrowers.get_mut(&user).unwrap();
                    borrower.collateral.insert(log.reserve);
                    borrower.is_dirty = true;
                    borrower.last_touched_block = latest_block.as_u64();
                    existing_collateral_users += 1;
                } else {
                    self.borrowers.insert(
                        user,
                        Borrower {
                            address: user,
                            collateral: HashSet::new(),
                            debt: HashSet::new(),
                            is_dirty: true,
                            last_touched_block: latest_block.as_u64(),
                            last_health_factor: None,
                            last_reconciled_block: None,
                        },
                    );
                    new_collateral_users += 1;
                }
            });
            
            info!("📈 Supply logs processed: {} new collateral users, {} existing users updated", 
                  new_collateral_users, existing_collateral_users);

            // 3. Process repay events (reduce debt)
            info!("📥 Fetching repay logs for {} blocks...", total_blocks);
            let repay_logs = self.get_repay_logs(self.last_block_number.into(), latest_block).await?;
            info!("📊 Found {} repay logs, processing debt reduction...", repay_logs.len());
            
            let mut repayments_processed = 0;
            repay_logs.into_iter().for_each(|log| {
                let user = log.user;
                if let Some(borrower) = self.borrowers.get_mut(&user) {
                    borrower.debt.remove(&log.reserve);
                    borrower.is_dirty = true;
                    borrower.last_touched_block = latest_block.as_u64();
                    repayments_processed += 1;
                    
                    // Check if borrower has no more debt
                    if borrower.debt.is_empty() && borrower.collateral.is_empty() {
                        borrowers_to_remove.push(user);
                    }
                }
            });
            
            info!("📉 Repay logs processed: {} repayments processed", repayments_processed);

            // 4. Process withdraw events (reduce collateral)
            info!("📥 Fetching withdraw logs for {} blocks...", total_blocks);
            let withdraw_logs = self.get_withdraw_logs(self.last_block_number.into(), latest_block).await?;
            info!("📊 Found {} withdraw logs, processing collateral reduction...", withdraw_logs.len());
            
            let mut withdrawals_processed = 0;
            withdraw_logs.into_iter().for_each(|log| {
                let user = log.user;
                if let Some(borrower) = self.borrowers.get_mut(&user) {
                    borrower.collateral.remove(&log.reserve);
                    borrower.is_dirty = true;
                    borrower.last_touched_block = latest_block.as_u64();
                    withdrawals_processed += 1;
                    
                    // Check if borrower has no more debt or collateral
                    if borrower.debt.is_empty() && borrower.collateral.is_empty() {
                        borrowers_to_remove.push(user);
                    }
                }
            });
            
            info!("📉 Withdraw logs processed: {} withdrawals processed", withdrawals_processed);

            // 5. Process liquidation events (remove liquidated borrowers)
            info!("📥 Fetching liquidation logs for {} blocks...", total_blocks);
            let liquidation_logs = self.get_liquidation_logs(self.last_block_number.into(), latest_block).await?;
            info!("📊 Found {} liquidation logs, processing liquidations...", liquidation_logs.len());
            
            let mut liquidations_processed = 0;
            liquidation_logs.into_iter().for_each(|log| {
                let user = log.user;
                if self.borrowers.contains_key(&user) {
                    borrowers_to_remove.push(user);
                    liquidations_processed += 1;
                }
            });
            
            info!("💥 Liquidation logs processed: {} liquidations processed", liquidations_processed);

            // 6. Remove inactive borrowers
            let initial_count = self.borrowers.len();
            let removed_count = borrowers_to_remove.len();
            for user in &borrowers_to_remove {
                self.borrowers.remove(user);
            }
            
            // 7. Final cleanup: remove any borrowers with no debt and no collateral
            let mut empty_borrowers = Vec::new();
            for (user, borrower) in &self.borrowers {
                if borrower.debt.is_empty() && borrower.collateral.is_empty() {
                    empty_borrowers.push(*user);
                }
            }
            
            for user in empty_borrowers {
                self.borrowers.remove(&user);
            }
            
            let final_count = self.borrowers.len();
            let total_removed = initial_count - final_count;
            
            info!("🧹 Cleanup complete: {} borrowers removed ({} liquidated, {} inactive)", 
                  total_removed, liquidations_processed, removed_count);
            
            info!("✅ State update complete. Total borrowers: {} (processed {} blocks, removed {} inactive)", 
                  final_count, total_blocks, total_removed);
            
            // Reconcile dirty borrowers if it's time
            if self.should_reconcile_borrowers() {
                match self.reconcile_dirty_borrowers(latest_block.as_u64()).await {
                    Ok(_) => info!("✅ Borrower reconciliation completed successfully"),
                    Err(e) => {
                        error!("❌ Borrower reconciliation failed: {}", e);
                        // Don't fail the entire state update, just log the error
                    }
                }
            }
            
            // Prune stale borrowers
            match self.prune_stale_borrowers(latest_block.as_u64()).await {
                Ok(_) => info!("✅ Borrower pruning completed successfully"),
                Err(e) => {
                    error!("❌ Borrower pruning failed: {}", e);
                    // Don't fail the entire state update, just log the error
                }
            }
        } else {
            info!("💤 No new blocks to process - blockchain is quiet");
        }

        // Persist updated state to storage
        let cache = StateCache {
            last_block_number: latest_block.as_u64(),
            borrowers: self.borrowers.clone(),
        };
        self.last_block_number = latest_block.as_u64();
        
        if let Err(e) = self.storage.save_cache(&cache).await {
            error!("Failed to save state cache to storage: {}", e);
        } else {
            info!("💾 State cache saved to storage ({} borrowers saved)", self.borrowers.len());
        }

        Ok(())
    }

    /// Fetches all borrow events from the specified block range
    /// 
    /// Processes blocks in chunks to respect RPC endpoint limits and provides
    /// progress information during long queries. Includes timeout handling
    /// to prevent hanging on slow RPC responses.
    /// 
    /// # Arguments
    /// * `from_block` - Starting block number (inclusive)
    /// * `to_block` - Ending block number (inclusive)
    /// 
    /// # Returns
    /// * `Result<Vec<BorrowFilter>>` - List of borrow events found
    async fn get_borrow_logs(&self, from_block: U64, to_block: U64) -> Result<Vec<BorrowFilter>> {
        let pool = Pool::<M>::new(self.config.pool_address, self.client.clone());

        let mut res = Vec::new();
        let total_chunks = ((to_block.as_u64() - from_block.as_u64()) / self.log_block_range) + 1;
        let mut current_chunk = 0;
        
        for start_block in
            (from_block.as_u64()..to_block.as_u64()).step_by(self.log_block_range as usize)
        {
            current_chunk += 1;
            let end_block = std::cmp::min(start_block + self.log_block_range - 1, to_block.as_u64());
            
            if current_chunk % 10 == 0 || current_chunk == total_chunks {
                info!("Processing borrow logs chunk {}/{} (blocks {}-{})", current_chunk, total_chunks, start_block, end_block);
            }
            
            info!("⏳ Waiting for RPC response for borrow logs (blocks {}-{})...", start_block, end_block);
            
            // Add delay between requests to respect rate limits
            if current_chunk > 1 {
                let delay = std::time::Duration::from_millis(self.request_delay_ms);
                tokio::time::sleep(delay).await;
            }
            
            let query_result = tokio::time::timeout(
                std::time::Duration::from_secs(30), // 30 second timeout per chunk
                pool.borrow_filter()
                    .from_block(start_block)
                    .to_block(end_block)
                    .address(ValueOrArray::Value(self.config.pool_address))
                    .query()
            ).await;
            
            let logs = match query_result {
                Ok(Ok(logs)) => logs,
                Ok(Err(e)) => {
                    error!("Error fetching borrow logs for blocks {}-{}: {}", start_block, end_block, e);
                    // Retry this block range instead of skipping
                    let mut retry_logs = Vec::new();
                    let mut retry_success = false;
                    
                    for attempt in 1..=self.retry_attempts {
                        let delay_ms = self.retry_delay_ms * (2_u64.pow(attempt - 1)); // Exponential backoff
                        warn!("⚠️ Retrying borrow logs for blocks {}-{} (attempt {}/{}), waiting {}ms...", 
                              start_block, end_block, attempt, self.retry_attempts, delay_ms);
                        
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        
                        match pool.borrow_filter()
                            .from_block(start_block)
                            .to_block(end_block)
                            .address(ValueOrArray::Value(self.config.pool_address))
                            .query()
                            .await {
                            Ok(logs) => {
                                retry_logs = logs;
                                retry_success = true;
                                info!("✅ Borrow logs retry succeeded on attempt {} for blocks {}-{}", attempt, start_block, end_block);
                                break;
                            }
                            Err(e) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch borrow logs for blocks {}-{} after {} retries: {}", 
                                           start_block, end_block, self.retry_attempts, e);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: {}", attempt, start_block, end_block, e);
                                }
                            }
                        }
                    }
                    
                    if !retry_success {
                        continue; // Skip this block range if all retries failed
                    }
                    
                    retry_logs
                }
                Err(_) => {
                    error!("Timeout fetching borrow logs for blocks {}-{}", start_block, end_block);
                    // Retry this block range instead of skipping
                    let mut retry_logs = Vec::new();
                    let mut retry_success = false;
                    
                    for attempt in 1..=self.retry_attempts {
                        let delay_ms = self.retry_delay_ms * (2_u64.pow(attempt - 1)); // Exponential backoff
                        warn!("⚠️ Retrying borrow logs for blocks {}-{} (attempt {}/{}), waiting {}ms...", 
                              start_block, end_block, attempt, self.retry_attempts, delay_ms);
                        
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(30),
                            pool.borrow_filter()
                                .from_block(start_block)
                                .to_block(end_block)
                                .address(ValueOrArray::Value(self.config.pool_address))
                                .query()
                        ).await {
                            Ok(Ok(logs)) => {
                                retry_logs = logs;
                                retry_success = true;
                                info!("✅ Borrow logs retry succeeded on attempt {} for blocks {}-{}", attempt, start_block, end_block);
                                break;
                            }
                            Ok(Err(e)) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch borrow logs for blocks {}-{} after {} retries: {}", 
                                           start_block, end_block, self.retry_attempts, e);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: {}", attempt, start_block, end_block, e);
                                }
                            }
                            Err(_) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch borrow logs for blocks {}-{} after {} retries: timeout", 
                                           start_block, end_block, self.retry_attempts);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: timeout", attempt, start_block, end_block);
                                }
                            }
                        }
                    }
                    
                    if !retry_success {
                        continue; // Skip this block range if all retries failed
                    }
                    
                    retry_logs
                }
            };
            
            logs.into_iter().for_each(|log| {
                res.push(log);
            });
        }

        Ok(res)
    }

    /// Fetches all supply events from the specified block range
    /// 
    /// Similar to borrow logs but tracks collateral deposits. Processes blocks
    /// in chunks with timeout handling for reliable RPC interaction.
    /// 
    /// # Arguments
    /// * `from_block` - Starting block number (inclusive)
    /// * `to_block` - Ending block number (inclusive)
    /// 
    /// # Returns
    /// * `Result<Vec<SupplyFilter>>` - List of supply events found
    async fn get_supply_logs(&self, from_block: U64, to_block: U64) -> Result<Vec<SupplyFilter>> {
        let pool = Pool::<M>::new(self.config.pool_address, self.client.clone());

        let mut res = Vec::new();
        let total_chunks = ((to_block.as_u64() - from_block.as_u64()) / self.log_block_range) + 1;
        let mut current_chunk = 0;
        
        for start_block in
            (from_block.as_u64()..to_block.as_u64()).step_by(self.log_block_range as usize)
        {
            current_chunk += 1;
            let end_block = std::cmp::min(start_block + self.log_block_range - 1, to_block.as_u64());
            
            if current_chunk % 10 == 0 || current_chunk == total_chunks {
                info!("Processing supply logs chunk {}/{} (blocks {}-{})", current_chunk, total_chunks, start_block, end_block);
            }
            
            info!("⏳ Waiting for RPC response for supply logs (blocks {}-{})...", start_block, end_block);
            
            // Add delay between requests to respect rate limits
            if current_chunk > 1 {
                let delay = std::time::Duration::from_millis(self.request_delay_ms);
                tokio::time::sleep(delay).await;
            }
            
            let query_result = tokio::time::timeout(
                std::time::Duration::from_secs(30), // 30 second timeout per chunk
                pool.supply_filter()
                    .from_block(start_block)
                    .to_block(end_block)
                    .address(ValueOrArray::Value(self.config.pool_address))
                    .query()
            ).await;
            
            let logs = match query_result {
                Ok(Ok(logs)) => logs,
                Ok(Err(e)) => {
                    error!("Error fetching supply logs for blocks {}-{}: {}", start_block, end_block, e);
                    // Retry this block range instead of skipping
                    let mut retry_logs = Vec::new();
                    let mut retry_success = false;
                    
                    for attempt in 1..=self.retry_attempts {
                        let delay_ms = self.retry_delay_ms * (2_u64.pow(attempt - 1)); // Exponential backoff
                        warn!("⚠️ Retrying supply logs for blocks {}-{} (attempt {}/{}), waiting {}ms...", 
                              start_block, end_block, attempt, self.retry_attempts, delay_ms);
                        
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        
                        match pool.supply_filter()
                            .from_block(start_block)
                            .to_block(end_block)
                            .address(ValueOrArray::Value(self.config.pool_address))
                            .query()
                            .await {
                            Ok(logs) => {
                                retry_logs = logs;
                                retry_success = true;
                                info!("✅ Supply logs retry succeeded on attempt {} for blocks {}-{}", attempt, start_block, end_block);
                                break;
                            }
                            Err(e) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch supply logs for blocks {}-{} after {} retries: {}", 
                                           start_block, end_block, self.retry_attempts, e);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: {}", attempt, start_block, end_block, e);
                                }
                            }
                        }
                    }
                    
                    if !retry_success {
                        continue; // Skip this block range if all retries failed
                    }
                    
                    retry_logs
                }
                Err(_) => {
                    error!("Timeout fetching supply logs for blocks {}-{}", start_block, end_block);
                    // Retry this block range instead of skipping
                    let mut retry_logs = Vec::new();
                    let mut retry_success = false;
                    
                    for attempt in 1..=self.retry_attempts {
                        let delay_ms = self.retry_delay_ms * (2_u64.pow(attempt - 1)); // Exponential backoff
                        warn!("⚠️ Retrying supply logs for blocks {}-{} (attempt {}/{}), waiting {}ms...", 
                              start_block, end_block, attempt, self.retry_attempts, delay_ms);
                        
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(30),
                            pool.supply_filter()
                                .from_block(start_block)
                                .to_block(end_block)
                                .address(ValueOrArray::Value(self.config.pool_address))
                                .query()
                        ).await {
                            Ok(Ok(logs)) => {
                                retry_logs = logs;
                                retry_success = true;
                                info!("✅ Supply logs retry succeeded on attempt {} for blocks {}-{}", attempt, start_block, end_block);
                                break;
                            }
                            Ok(Err(e)) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch supply logs for blocks {}-{} after {} retries: {}", 
                                           start_block, end_block, self.retry_attempts, e);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: {}", attempt, start_block, end_block, e);
                                }
                            }
                            Err(_) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch supply logs for blocks {}-{} after {} retries: timeout", 
                                           start_block, end_block, self.retry_attempts);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: timeout", attempt, start_block, end_block);
                                }
                            }
                        }
                    }
                    
                    if !retry_success {
                        continue; // Skip this block range if all retries failed
                    }
                    
                    retry_logs
                }
            };
            
            logs.into_iter().for_each(|log| {
                res.push(log);
            });
        }

        Ok(res)
    }

    /// Fetches all repay events from the specified block range
    /// 
    /// Tracks debt reduction events when users repay their loans. Processes blocks
    /// in chunks with timeout handling for reliable RPC interaction.
    /// 
    /// # Arguments
    /// * `from_block` - Starting block number (inclusive)
    /// * `to_block` - Ending block number (inclusive)
    /// 
    /// # Returns
    /// * `Result<Vec<RepayFilter>>` - List of repay events found
    async fn get_repay_logs(&self, from_block: U64, to_block: U64) -> Result<Vec<RepayFilter>> {
        let pool = Pool::<M>::new(self.config.pool_address, self.client.clone());

        let mut res = Vec::new();
        let total_chunks = ((to_block.as_u64() - from_block.as_u64()) / self.log_block_range) + 1;
        let mut current_chunk = 0;
        
        for start_block in
            (from_block.as_u64()..to_block.as_u64()).step_by(self.log_block_range as usize)
        {
            current_chunk += 1;
            let end_block = std::cmp::min(start_block + self.log_block_range - 1, to_block.as_u64());
            
            if current_chunk % 10 == 0 || current_chunk == total_chunks {
                info!("Processing repay logs chunk {}/{} (blocks {}-{})", current_chunk, total_chunks, start_block, end_block);
            }
            
            info!("⏳ Waiting for RPC response for repay logs (blocks {}-{})...", start_block, end_block);
            let query_result = tokio::time::timeout(
                std::time::Duration::from_secs(30), // 30 second timeout per chunk
                pool.repay_filter()
                    .from_block(start_block)
                    .to_block(end_block)
                    .address(ValueOrArray::Value(self.config.pool_address))
                    .query()
            ).await;
            
            let logs = match query_result {
                Ok(Ok(logs)) => logs,
                Ok(Err(e)) => {
                    error!("Error fetching repay logs for blocks {}-{}: {}", start_block, end_block, e);
                    // Retry this block range instead of skipping
                    let mut retry_logs = Vec::new();
                    let mut retry_success = false;
                    
                    for attempt in 1..=self.retry_attempts {
                        let delay_ms = self.retry_delay_ms * (2_u64.pow(attempt - 1)); // Exponential backoff
                        warn!("⚠️ Retrying repay logs for blocks {}-{} (attempt {}/{}), waiting {}ms...", 
                              start_block, end_block, attempt, self.retry_attempts, delay_ms);
                        
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        
                        match pool.repay_filter()
                            .from_block(start_block)
                            .to_block(end_block)
                            .address(ValueOrArray::Value(self.config.pool_address))
                            .query()
                            .await {
                            Ok(logs) => {
                                retry_logs = logs;
                                retry_success = true;
                                info!("✅ Repay logs retry succeeded on attempt {} for blocks {}-{}", attempt, start_block, end_block);
                                break;
                            }
                            Err(e) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch repay logs for blocks {}-{} after {} retries: {}", 
                                           start_block, end_block, self.retry_attempts, e);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: {}", attempt, start_block, end_block, e);
                                }
                            }
                        }
                    }
                    
                    if !retry_success {
                        continue; // Skip this block range if all retries failed
                    }
                    
                    retry_logs
                }
                Err(_) => {
                    error!("Timeout fetching repay logs for blocks {}-{}", start_block, end_block);
                    // Retry this block range instead of skipping
                    let mut retry_logs = Vec::new();
                    let mut retry_success = false;
                    
                    for attempt in 1..=self.retry_attempts {
                        let delay_ms = self.retry_delay_ms * (2_u64.pow(attempt - 1)); // Exponential backoff
                        warn!("⚠️ Retrying repay logs for blocks {}-{} (attempt {}/{}), waiting {}ms...", 
                              start_block, end_block, attempt, self.retry_attempts, delay_ms);
                        
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(30),
                            pool.repay_filter()
                                .from_block(start_block)
                                .to_block(end_block)
                                .address(ValueOrArray::Value(self.config.pool_address))
                                .query()
                        ).await {
                            Ok(Ok(logs)) => {
                                retry_logs = logs;
                                retry_success = true;
                                info!("✅ Repay logs retry succeeded on attempt {} for blocks {}-{}", attempt, start_block, end_block);
                                break;
                            }
                            Ok(Err(e)) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch repay logs for blocks {}-{} after {} retries: {}", 
                                           start_block, end_block, self.retry_attempts, e);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: {}", attempt, start_block, end_block, e);
                                }
                            }
                            Err(_) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch repay logs for blocks {}-{} after {} retries: timeout", 
                                           start_block, end_block, self.retry_attempts);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: timeout", attempt, start_block, end_block);
                                }
                            }
                        }
                    }
                    
                    if !retry_success {
                        continue; // Skip this block range if all retries failed
                    }
                    
                    retry_logs
                }
            };
            
            logs.into_iter().for_each(|log| {
                res.push(log);
            });
        }

        Ok(res)
    }

    /// Fetches all withdraw events from the specified block range
    /// 
    /// Tracks collateral reduction events when users withdraw their deposits.
    /// Processes blocks in chunks with timeout handling for reliable RPC interaction.
    /// 
    /// # Arguments
    /// * `from_block` - Starting block number (inclusive)
    /// * `to_block` - Ending block number (inclusive)
    /// 
    /// # Returns
    /// * `Result<Vec<WithdrawFilter>>` - List of withdraw events found
    async fn get_withdraw_logs(&self, from_block: U64, to_block: U64) -> Result<Vec<WithdrawFilter>> {
        let pool = Pool::<M>::new(self.config.pool_address, self.client.clone());

        let mut res = Vec::new();
        let total_chunks = ((to_block.as_u64() - from_block.as_u64()) / self.log_block_range) + 1;
        let mut current_chunk = 0;
        
        for start_block in
            (from_block.as_u64()..to_block.as_u64()).step_by(self.log_block_range as usize)
        {
            current_chunk += 1;
            let end_block = std::cmp::min(start_block + self.log_block_range - 1, to_block.as_u64());
            
            if current_chunk % 10 == 0 || current_chunk == total_chunks {
                info!("Processing withdraw logs chunk {}/{} (blocks {}-{})", current_chunk, total_chunks, start_block, end_block);
            }
            
            info!("⏳ Waiting for RPC response for withdraw logs (blocks {}-{})...", start_block, end_block);
            let query_result = tokio::time::timeout(
                std::time::Duration::from_secs(30), // 30 second timeout per chunk
                pool.withdraw_filter()
                    .from_block(start_block)
                    .to_block(end_block)
                    .address(ValueOrArray::Value(self.config.pool_address))
                    .query()
            ).await;
            
            let logs = match query_result {
                Ok(Ok(logs)) => logs,
                Ok(Err(e)) => {
                    error!("Error fetching withdraw logs for blocks {}-{}: {}", start_block, end_block, e);
                    // Retry this block range instead of skipping
                    let mut retry_logs = Vec::new();
                    let mut retry_success = false;
                    
                    for attempt in 1..=self.retry_attempts {
                        let delay_ms = self.retry_delay_ms * (2_u64.pow(attempt - 1)); // Exponential backoff
                        warn!("⚠️ Retrying withdraw logs for blocks {}-{} (attempt {}/{}), waiting {}ms...", 
                              start_block, end_block, attempt, self.retry_attempts, delay_ms);
                        
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        
                        match pool.withdraw_filter()
                            .from_block(start_block)
                            .to_block(end_block)
                            .address(ValueOrArray::Value(self.config.pool_address))
                            .query()
                            .await {
                            Ok(logs) => {
                                retry_logs = logs;
                                retry_success = true;
                                info!("✅ Withdraw logs retry succeeded on attempt {} for blocks {}-{}", attempt, start_block, end_block);
                                break;
                            }
                            Err(e) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch withdraw logs for blocks {}-{} after {} retries: {}", 
                                           start_block, end_block, self.retry_attempts, e);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: {}", attempt, start_block, end_block, e);
                                }
                            }
                        }
                    }
                    
                    if !retry_success {
                        continue; // Skip this block range if all retries failed
                    }
                    
                    retry_logs
                }
                Err(_) => {
                    error!("Timeout fetching withdraw logs for blocks {}-{}", start_block, end_block);
                    // Retry this block range instead of skipping
                    let mut retry_logs = Vec::new();
                    let mut retry_success = false;
                    
                    for attempt in 1..=self.retry_attempts {
                        let delay_ms = self.retry_delay_ms * (2_u64.pow(attempt - 1)); // Exponential backoff
                        warn!("⚠️ Retrying withdraw logs for blocks {}-{} (attempt {}/{}), waiting {}ms...", 
                              start_block, end_block, attempt, self.retry_attempts, delay_ms);
                        
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(30),
                            pool.withdraw_filter()
                                .from_block(start_block)
                                .to_block(end_block)
                                .address(ValueOrArray::Value(self.config.pool_address))
                                .query()
                        ).await {
                            Ok(Ok(logs)) => {
                                retry_logs = logs;
                                retry_success = true;
                                info!("✅ Withdraw logs retry succeeded on attempt {} for blocks {}-{}", attempt, start_block, end_block);
                                break;
                            }
                            Ok(Err(e)) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch withdraw logs for blocks {}-{} after {} retries: {}", 
                                           start_block, end_block, self.retry_attempts, e);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: {}", attempt, start_block, end_block, e);
                                }
                            }
                            Err(_) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch withdraw logs for blocks {}-{} after {} retries: timeout", 
                                           start_block, end_block, self.retry_attempts);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: timeout", attempt, start_block, end_block);
                                }
                            }
                        }
                    }
                    
                    if !retry_success {
                        continue; // Skip this block range if all retries failed
                    }
                    
                    retry_logs
                }
            };
            
            logs.into_iter().for_each(|log| {
                res.push(log);
            });
        }

        Ok(res)
    }

    /// Fetches all liquidation events from the specified block range
    /// 
    /// Tracks when users are liquidated by other liquidators. These events
    /// help maintain accurate borrower state by removing liquidated positions.
    /// 
    /// # Arguments
    /// * `from_block` - Starting block number (inclusive)
    /// * `to_block` - Ending block number (inclusive)
    /// 
    /// # Returns
    /// * `Result<Vec<LiquidationCallFilter>>` - List of liquidation events found
    async fn get_liquidation_logs(&self, from_block: U64, to_block: U64) -> Result<Vec<LiquidationCallFilter>> {
        let pool = Pool::<M>::new(self.config.pool_address, self.client.clone());

        let mut res = Vec::new();
        let total_chunks = ((to_block.as_u64() - from_block.as_u64()) / self.log_block_range) + 1;
        let mut current_chunk = 0;
        
        for start_block in
            (from_block.as_u64()..to_block.as_u64()).step_by(self.log_block_range as usize)
        {
            current_chunk += 1;
            let end_block = std::cmp::min(start_block + self.log_block_range - 1, to_block.as_u64());
            
            if current_chunk % 10 == 0 || current_chunk == total_chunks {
                info!("Processing liquidation logs chunk {}/{} (blocks {}-{})", current_chunk, total_chunks, start_block, end_block);
            }
            
            info!("⏳ Waiting for RPC response for liquidation logs (blocks {}-{})...", start_block, end_block);
            let query_result = tokio::time::timeout(
                std::time::Duration::from_secs(30), // 30 second timeout per chunk
                pool.liquidation_call_filter()
                    .from_block(start_block)
                    .to_block(end_block)
                    .address(ValueOrArray::Value(self.config.pool_address))
                    .query()
            ).await;
            
            let logs = match query_result {
                Ok(Ok(logs)) => logs,
                Ok(Err(e)) => {
                    error!("Error fetching liquidation logs for blocks {}-{}: {}", start_block, end_block, e);
                    // Retry this block range instead of skipping
                    let mut retry_logs = Vec::new();
                    let mut retry_success = false;
                    
                    for attempt in 1..=self.retry_attempts {
                        let delay_ms = self.retry_delay_ms * (2_u64.pow(attempt - 1)); // Exponential backoff
                        warn!("⚠️ Retrying liquidation logs for blocks {}-{} (attempt {}/{}), waiting {}ms...", 
                              start_block, end_block, attempt, self.retry_attempts, delay_ms);
                        
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        
                        match pool.liquidation_call_filter()
                            .from_block(start_block)
                            .to_block(end_block)
                            .address(ValueOrArray::Value(self.config.pool_address))
                            .query()
                            .await {
                            Ok(logs) => {
                                retry_logs = logs;
                                retry_success = true;
                                info!("✅ Liquidation logs retry succeeded on attempt {} for blocks {}-{}", attempt, start_block, end_block);
                                break;
                            }
                            Err(e) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch liquidation logs for blocks {}-{} after {} retries: {}", 
                                           start_block, end_block, self.retry_attempts, e);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: {}", attempt, start_block, end_block, e);
                                }
                            }
                        }
                    }
                    
                    if !retry_success {
                        continue; // Skip this block range if all retries failed
                    }
                    
                    retry_logs
                }
                Err(_) => {
                    error!("Timeout fetching liquidation logs for blocks {}-{}", start_block, end_block);
                    // Retry this block range instead of skipping
                    let mut retry_logs = Vec::new();
                    let mut retry_success = false;
                    
                    for attempt in 1..=self.retry_attempts {
                        let delay_ms = self.retry_delay_ms * (2_u64.pow(attempt - 1)); // Exponential backoff
                        warn!("⚠️ Retrying liquidation logs for blocks {}-{} (attempt {}/{}), waiting {}ms...", 
                              start_block, end_block, attempt, self.retry_attempts, delay_ms);
                        
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(30),
                            pool.liquidation_call_filter()
                                .from_block(start_block)
                                .to_block(end_block)
                                .address(ValueOrArray::Value(self.config.pool_address))
                                .query()
                        ).await {
                            Ok(Ok(logs)) => {
                                retry_logs = logs;
                                retry_success = true;
                                info!("✅ Liquidation logs retry succeeded on attempt {} for blocks {}-{}", attempt, start_block, end_block);
                                break;
                            }
                            Ok(Err(e)) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch liquidation logs for blocks {}-{} after {} retries: {}", 
                                           start_block, end_block, self.retry_attempts, e);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: {}", attempt, start_block, end_block, e);
                                }
                            }
                            Err(_) => {
                                if attempt == self.retry_attempts {
                                    error!("❌ Failed to fetch liquidation logs for blocks {}-{} after {} retries: timeout", 
                                           start_block, end_block, self.retry_attempts);
                                } else {
                                    warn!("⚠️ Retry attempt {} failed for blocks {}-{}: timeout", attempt, start_block, end_block);
                                }
                            }
                        }
                    }
                    
                    if !retry_success {
                        continue; // Skip this block range if all retries failed
                    }
                    
                    retry_logs
                }
            };
            
            logs.into_iter().for_each(|log| {
                res.push(log);
            });
        }

        Ok(res)
    }

    /// Skips token approvals since the liquidator uses flash loans
    /// 
    /// The liquidator contract executes liquidations using Uniswap V3 flash loans,
    /// which means it doesn't need to hold tokens or have pre-approvals.
    /// This method is kept for interface compatibility but performs no operations.
    /// 
    /// # Returns
    /// * `Result<()>` - Always succeeds
    async fn approve_tokens(&mut self) -> Result<()> {
        info!("Skipping token approvals - liquidator uses flash loans, no pre-approvals needed");
        Ok(())
    }

    /// Updates token configurations from the blockchain
    /// 
    /// Fetches all reserve tokens and their associated aTokens from the pool,
    /// along with their risk parameters (LTV, liquidation threshold, bonus, etc.).
    /// This information is essential for calculating liquidation opportunities.
    /// 
    /// # Returns
    /// * `Result<()>` - Success or error from token configuration update
    async fn update_token_configs(&mut self) -> Result<()> {
        let pool_data =
            IPoolDataProvider::<M>::new(self.config.pool_data_provider, self.client.clone());
        let all_tokens = pool_data.get_all_reserves_tokens().await?;
        let all_a_tokens = pool_data.get_all_a_tokens().await?;
        info!("all_tokens: {:?}", all_tokens);
        for (token, a_token) in zip(all_tokens, all_a_tokens) {
            let (decimals, ltv, threshold, bonus, reserve, _, _, _, _, _) = pool_data
                .get_reserve_configuration_data(token.token_address)
                .await?;
            let protocol_fee = pool_data
                .get_liquidation_protocol_fee(token.token_address)
                .await?;
            self.tokens.insert(
                token.token_address,
                TokenConfig {
                    address: token.token_address,
                    a_address: a_token.token_address,
                    decimals: decimals.low_u64(),
                    ltv: ltv.low_u64(),
                    liquidation_threshold: threshold.low_u64(),
                    liquidation_bonus: bonus.low_u64(),
                    reserve_factor: reserve.low_u64(),
                    protocol_fee: protocol_fee.low_u64(),
                },
            );
        }

        Ok(())
    }

    /// Converts asset prices from USD to ETH terms
    /// 
    /// Uses 8 decimal precision for price calculations. WETH is treated as 1:1
    /// with ETH, while other assets are converted using their USD prices.
    /// 
    /// # Arguments
    /// * `asset` - Address of the asset to price
    /// * `pool_state` - Current pool state containing price information
    /// 
    /// # Returns
    /// * `Result<U256>` - Asset price in ETH terms with 8 decimal precision
    async fn get_asset_price_eth(&self, asset: &Address, pool_state: &PoolState) -> Result<U256> {
        // WETH is always 1:1 with ETH
        if asset.eq(&self.weth_address) {
            return Ok(U256::from(PRICE_ONE));
        }

        // Get USD price of the target asset
        let usd_price = pool_state
            .prices
            .get(asset)
            .ok_or(anyhow!("No price found for asset {}", asset.to_string()))?;
        
        // Get USD price of WETH (ETH)
        let usd_price_eth = pool_state.prices.get(&self.weth_address).ok_or(anyhow!(
            "No price found for asset {}",
            self.weth_address.to_string()
        ))?;
        
        // Convert: (USD/token) * (ETH/USD) = ETH/token
        Ok(usd_price * U256::from(PRICE_ONE) / usd_price_eth)
    }

    /// Finds the most profitable liquidation opportunity among underwater borrowers
    /// 
    /// Scans all borrowers with health factor < 1.0 and calculates potential
    /// profit for each liquidation opportunity. Returns the most profitable one.
    /// 
    /// # Returns
    /// * `Result<Option<LiquidationOpportunity>>` - Best opportunity found, or None if none exist
    async fn get_best_liquidation_op(&mut self) -> Result<Option<LiquidationOpportunity>> {
        let underwater = self.get_underwater_borrowers().await?;

        if underwater.len() == 0 {
            return Ok(None); // No underwater borrowers is normal, not an error
        }

        info!("Found {} underwater borrowers", underwater.len());
        let pool_data =
            IPoolDataProvider::<M>::new(self.config.pool_data_provider, self.client.clone());

        let mut best_bonus: I256 = I256::MIN;
        let mut best_op: Option<LiquidationOpportunity> = None;
        let pool_state = self.get_pool_state().await?;

        for (borrower, health_factor) in underwater {
            if let Some(op) = self
                .get_liquidation_opportunity(
                    self.borrowers
                        .get(&borrower)
                        .ok_or(anyhow!("Borrower not found"))?,
                    &pool_data,
                    &health_factor,
                    &pool_state,
                )
                .await
                .map_err(|e| info!("Liquidation op failed {}", e))
                .ok()
            {
                if op.profit_eth > best_bonus {
                    best_bonus = op.profit_eth;
                    best_op = Some(op);
                }
            }
        }

        Ok(best_op)
    }

    /// Fetches current pool state including all asset prices
    /// 
    /// Uses multicall to efficiently query price oracle for all supported tokens
    /// in a single RPC call, minimizing gas costs and improving performance.
    /// 
    /// # Returns
    /// * `Result<PoolState>` - Current pool state with asset prices
    async fn get_pool_state(&self) -> Result<PoolState> {
        let mut multicall = Multicall::<M>::new(
            self.client.clone(),
            Some(H160::from_str(
                "0xcA11bde05977b3631167028862bE2a173976CA11",
            )?),
        )
        .await?;
        let mut prices = HashMap::new();
        let price_oracle = IAaveOracle::<M>::new(self.config.oracle_address, self.client.clone());

        // Add price queries for all tokens to multicall
        for token_address in self.tokens.keys() {
            multicall.add_call(price_oracle.get_asset_price(*token_address), false);
        }

        // Execute all price queries in a single call
        let result: Vec<U256> = multicall.call_array().await?;
        for (token_address, price) in zip(self.tokens.keys(), result) {
            prices.insert(*token_address, price);
        }
        multicall.clear_calls();

        Ok(PoolState { prices })
    }

    /// Calculates liquidation opportunity for a specific borrower
    /// 
    /// Determines the optimal amount of debt to cover and collateral to liquidate
    /// based on the borrower's health factor and liquidation parameters. Calculates
    /// expected profit in ETH terms.
    /// 
    /// # Arguments
    /// * `borrower` - Borrower information including collateral and debt
    /// * `pool_data` - Pool data provider for reserve information
    /// * `health_factor` - Current health factor of the borrower
    /// * `pool_state` - Current pool state with asset prices
    /// 
    /// # Returns
    /// * `Result<LiquidationOpportunity>` - Calculated liquidation opportunity
    async fn get_liquidation_opportunity(
        &self,
        borrower: &Borrower,
        pool_data: &IPoolDataProvider<M>,
        health_factor: &U256,
        pool_state: &PoolState,
    ) -> Result<LiquidationOpportunity> {
        let Borrower {
            address: borrower_address,
            collateral,
            debt,
            is_dirty: _,
            last_touched_block: _,
            last_health_factor: _,
            last_reconciled_block: _,
        } = borrower;
        
        // TODO: Handle users with multiple collateral/debt positions
        // For now, use the first item from each set
        let collateral_address = collateral
            .iter()
            .next()
            .ok_or(anyhow!("No collateral found"))?;
        let debt_address = debt.iter().next().ok_or(anyhow!("No debt found"))?;
        
        // Get asset prices and configurations
        let collateral_asset_price = pool_state
            .prices
            .get(collateral_address)
            .ok_or(anyhow!("No collateral price"))?;
        let debt_asset_price = pool_state
            .prices
            .get(debt_address)
            .ok_or(anyhow!("No debt price"))?;
        let collateral_config = self
            .tokens
            .get(collateral_address)
            .ok_or(anyhow!("Failed to get collateral address"))?;
        let debt_config = self
            .tokens
            .get(debt_address)
            .ok_or(anyhow!("Failed to get debt address"))?;
        
        // Calculate units for precision handling
        let collateral_unit = U256::from(10).pow(collateral_config.decimals.into());
        let debt_unit = U256::from(10).pow(debt_config.decimals.into());
        let liquidation_bonus = collateral_config.liquidation_bonus;
        let a_token = IERC20::new(collateral_config.a_address.clone(), self.client.clone());

        // Get user's debt amounts for the specific asset
        let (_, stable_debt, variable_debt, _, _, _, _, _, _) = pool_data
            .get_user_reserve_data(*debt_address, *borrower_address)
            .await?;
        
        // Determine close factor based on health factor
        let close_factor = if health_factor.gt(&self.close_factor_threshold) {
            self.default_close_factor
        } else {
            self.max_close_factor
        };

        // Calculate debt to cover and collateral to liquidate
        let mut debt_to_cover =
            (stable_debt + variable_debt) * close_factor / self.max_close_factor;
        let base_collateral = (debt_asset_price * debt_to_cover * collateral_unit)
            / (collateral_asset_price * debt_unit);
        let mut collateral_to_liquidate = percent_mul(base_collateral, liquidation_bonus);
        let user_collateral_balance = a_token.balance_of(*borrower_address).await?;

        // Adjust amounts if user has insufficient collateral
        if collateral_to_liquidate > user_collateral_balance {
            collateral_to_liquidate = user_collateral_balance;
            debt_to_cover = (collateral_asset_price * collateral_to_liquidate * debt_unit)
                / percent_div(debt_asset_price * collateral_unit, liquidation_bonus);
        }

        // Create liquidation opportunity structure
        let mut op = LiquidationOpportunity {
            borrower: borrower_address.clone(),
            collateral: collateral_address.clone(),
            debt: debt_address.clone(),
            debt_to_cover,
            profit_eth: I256::from(0),
        };

        // Calculate actual profit by simulating the liquidation
        let gain = self.build_liquidation_call(&op).await?.call().await?;

        // Convert profit to ETH terms
        let weth_price = self
            .get_asset_price_eth(collateral_address, pool_state)
            .await?;
        op.profit_eth = gain * I256::from_dec_str(&weth_price.to_string())? / I256::from(PRICE_ONE);

        info!(
            "Found opportunity - collateral: {:?}, debt: {:?}, collateral_to_liquidate: {:?}, debt_to_cover: {:?}, profit_eth: {:?}",
            collateral_address, debt_address, collateral_to_liquidate, debt_to_cover, op.profit_eth
        );

        Ok(op)
    }

    async fn build_liquidation_call(
        &self,
        op: &LiquidationOpportunity,
    ) -> Result<ContractCall<M, I256>> {
        let liquidator = Liquidator::new(self.liquidator, self.client.clone());
        let encoder = L2Encoder::new(self.config.l2_encoder, self.client.clone());
        let (data0, data1) = encoder
            .encode_liquidation_call(op.collateral, op.debt, op.borrower, op.debt_to_cover, false)
            .call()
            .await?;

        // TODO: handle arbitrary pool fees
        Ok(liquidator.liquidate(op.collateral, op.debt, 500, op.debt_to_cover, data0, data1))
    }

    async fn build_liquidation(&self, op: &LiquidationOpportunity) -> Result<TypedTransaction> {
        let mut call = self.build_liquidation_call(op).await?;
        Ok(call.tx.set_chain_id(self.chain_id).clone())
    }
    
    /// Marks a borrower as dirty (needs reconciliation)
    async fn mark_borrower_dirty(&mut self, address: Address, block_number: u64) -> Result<()> {
        if let Some(borrower) = self.borrowers.get_mut(&address) {
            borrower.is_dirty = true;
            borrower.last_touched_block = block_number;
        }
        
        // Also mark in storage
        self.storage.mark_borrower_dirty(&address, block_number).await?;
        
        Ok(())
    }
    
    /// Reconciles a borrower's position from the blockchain
    async fn reconcile_borrower(&mut self, address: Address, current_block: u64) -> Result<()> {
        info!("🔄 Reconciling borrower {} from chain", format!("{:?}", address));
        
        // Add timeout to prevent hanging
        let timeout_duration = time::Duration::from_secs(30); // 30 second timeout
        
        // Get user account data from the pool with timeout
        let pool = Pool::<M>::new(self.config.pool_address, self.client.clone());
        let user_data = match tokio::time::timeout(timeout_duration, pool.get_user_account_data(address).call()).await {
            Ok(result) => result?,
            Err(_) => {
                return Err(anyhow!("Timeout getting user account data for borrower {}", format!("{:?}", address)));
            }
        };
        
        // Check if user has any positions
        // user_data is (totalCollateralBase, totalDebtBase, availableBorrowsBase, currentLiquidationThreshold, ltv, healthFactor)
        if user_data.1 == U256::zero() && user_data.0 == U256::zero() {
            // User has no positions, remove them
            info!("🗑️ Borrower {} has no positions, removing from tracking", format!("{:?}", address));
            self.borrowers.remove(&address);
            self.storage.remove_borrower(&address).await?;
            return Ok(());
        }
        
        // Get detailed reserve data for each asset the user might have
        let pool_data_provider = IPoolDataProvider::<M>::new(self.config.pool_data_provider, self.client.clone());
        let reserves_tokens = match tokio::time::timeout(timeout_duration, pool_data_provider.get_all_reserves_tokens().call()).await {
            Ok(result) => result?,
            Err(_) => {
                return Err(anyhow!("Timeout getting reserves list for borrower {}", format!("{:?}", address)));
            }
        };
        
        let mut new_collateral = HashSet::new();
        let mut new_debt = HashSet::new();
        
        for token_data in reserves_tokens {
            match tokio::time::timeout(timeout_duration, pool_data_provider.get_user_reserve_data(token_data.token_address, address).call()).await {
                Ok(result) => {
                    match result {
                        Ok(user_reserve_data) => {
                            // Check if user has collateral in this reserve
                            // user_reserve_data is (currentATokenBalance, currentStableDebt, currentVariableDebt, principalStableDebt, scaledVariableDebt, stableBorrowRate, oldStableBorrowRate, stableBorrowLastUpdatedTimestamp, usageAsCollateralEnabledOnUser)
                            if user_reserve_data.0 > U256::zero() {
                                new_collateral.insert(token_data.token_address);
                            }
                            
                            // Check if user has debt in this reserve
                            if user_reserve_data.1 > U256::zero() || user_reserve_data.2 > U256::zero() {
                                new_debt.insert(token_data.token_address);
                            }
                        }
                        Err(e) => {
                            warn!("⚠️ Failed to get reserve data for token {}: {}, skipping", format!("{:?}", token_data.token_address), e);
                            // Continue with other tokens
                        }
                    }
                }
                Err(_) => {
                    warn!("⚠️ Timeout getting reserve data for token {}, skipping", format!("{:?}", token_data.token_address));
                    // Continue with other tokens
                }
            }
        }
        
        // Update borrower state
        if let Some(borrower) = self.borrowers.get_mut(&address) {
            borrower.collateral = new_collateral;
            borrower.debt = new_debt;
            borrower.is_dirty = false;
            borrower.last_reconciled_block = Some(current_block);
            
            // Update health factor if available (health factor is in basis points, e.g., 10000 = 1.0)
            if user_data.5 > U256::zero() {
                // Safely convert health factor, handling potential overflow
                match TryInto::<u128>::try_into(user_data.5) {
                    Ok(hf_u128) => {
                        // Clamp to reasonable range to avoid overflow
                        let clamped_hf = hf_u128.min(u128::MAX / 1000);
                        borrower.last_health_factor = Some(clamped_hf as u64);
                    }
                    Err(_) => {
                        warn!("⚠️ Health factor overflow for borrower {}, skipping", format!("{:?}", address));
                        borrower.last_health_factor = None;
                    }
                }
            }
            
            // Update in storage
            self.storage.update_borrower(borrower).await?;
            self.storage.mark_borrower_reconciled(&address, current_block).await?;
            
            info!("✅ Reconciled borrower {}: {} collateral, {} debt assets", 
                  format!("{:?}", address), borrower.collateral.len(), borrower.debt.len());
        }
        
        Ok(())
    }
    
    /// Reconciles all dirty borrowers
    async fn reconcile_dirty_borrowers(&mut self, current_block: u64) -> Result<()> {
        let dirty_borrowers = self.storage.get_dirty_borrowers().await?;
        
        if dirty_borrowers.is_empty() {
            return Ok(());
        }
        
        info!("🔄 Reconciling {} dirty borrowers", dirty_borrowers.len());
        
        // Limit the number of borrowers to reconcile per cycle to avoid RPC overload
        let max_reconcile = self.borrower_config.max_reconcile_per_cycle;
        let borrowers_to_reconcile = dirty_borrowers.iter().take(max_reconcile).cloned().collect::<Vec<_>>();
        
        let mut success_count = 0;
        let mut error_count = 0;
        
        for (index, address) in borrowers_to_reconcile.iter().enumerate() {
            // Circuit breaker: if we have too many consecutive failures, stop
            if error_count >= 5 && success_count == 0 {
                error!("🚨 Circuit breaker triggered: too many consecutive reconciliation failures");
                break;
            }
            
            match self.reconcile_borrower(*address, current_block).await {
                Ok(_) => {
                    success_count += 1;
                    error_count = 0; // Reset error count on success
                    
                    // Add delay between reconciliations to avoid RPC throttling
                    if self.request_delay_ms > 0 {
                        time::sleep(time::Duration::from_millis(self.request_delay_ms)).await;
                    }
                    
                    // Progress indicator for large batches
                    if borrowers_to_reconcile.len() > 10 && (index + 1) % 10 == 0 {
                        info!("🔄 Reconciliation progress: {}/{} completed", index + 1, borrowers_to_reconcile.len());
                    }
                }
                Err(e) => {
                    error_count += 1;
                    error!("❌ Failed to reconcile borrower {}: {}", format!("{:?}", address), e);
                    
                    // Add longer delay on errors to avoid overwhelming the system
                    if self.request_delay_ms > 0 {
                        time::sleep(time::Duration::from_millis(self.request_delay_ms * 2)).await;
                    }
                    
                    // Continue with other borrowers unless circuit breaker is triggered
                }
            }
        }
        
        info!("✅ Reconciliation complete: {} successful, {} failed", success_count, error_count);
        Ok(())
    }
    
    /// Prunes stale borrowers based on TTL
    async fn prune_stale_borrowers(&mut self, current_block: u64) -> Result<()> {
        if !self.borrower_config.enable_auto_pruning {
            return Ok(());
        }
        
        let ttl_blocks = self.borrower_config.stale_borrower_ttl_blocks;
        let stale_borrowers = self.storage.get_stale_borrowers(current_block, ttl_blocks).await?;
        
        if stale_borrowers.is_empty() {
            return Ok(());
        }
        
        info!("🗑️ Pruning {} stale borrowers (TTL: {} blocks)", stale_borrowers.len(), ttl_blocks);
        
        for address in stale_borrowers {
            // Quick check if borrower still has positions
            let pool = Pool::<M>::new(self.config.pool_address, self.client.clone());
            let user_data = pool.get_user_account_data(address).call().await?;
            
            // user_data is (totalCollateralBase, totalDebtBase, availableBorrowsBase, currentLiquidationThreshold, ltv, healthFactor)
            if user_data.1 == U256::zero() && user_data.0 == U256::zero() {
                // Borrower has no positions, safe to remove
                self.borrowers.remove(&address);
                self.storage.remove_borrower(&address).await?;
                info!("🗑️ Pruned stale borrower {} (no positions)", format!("{:?}", address));
            } else {
                // Borrower still has positions, mark as dirty for reconciliation
                self.mark_borrower_dirty(address, current_block).await?;
                info!("🔄 Marked stale borrower {} as dirty (has positions)", format!("{:?}", address));
            }
            
            // Add delay between checks to avoid RPC throttling
            if self.request_delay_ms > 0 {
                time::sleep(time::Duration::from_millis(self.request_delay_ms)).await;
            }
        }
        
        Ok(())
    }
    
    /// Shows a summary of the current bot state and resume information
    pub fn show_state_summary(&self) {
        info!("📊 === BOT STATE SUMMARY ===");
        info!("📍 Configured start block: {} (from bot.toml)", self.config.creation_block);
        info!("📍 Last processed block: {}", self.last_block_number);
        info!("📍 Total borrowers tracked: {}", self.borrowers.len());
        info!("📍 Dirty borrowers: {}", self.borrowers.values().filter(|b| b.is_dirty).count());
        info!("📊 === END SUMMARY ===");
    }
    
    /// Determines if it's time to reconcile borrowers based on configuration
    fn should_reconcile_borrowers(&self) -> bool {
        let reconcile_interval = self.borrower_config.reconcile_interval_blocks;
        if reconcile_interval == 0 {
            return true; // Reconcile every block
        }
        
        // Check if we've processed enough blocks since last reconciliation
        // For now, we'll reconcile every time update_state is called
        // In a more sophisticated implementation, we could track the last reconciliation block
        true
    }
}

fn percent_mul(a: U256, bps: u64) -> U256 {
    (U256::from(5000) + (a * bps)) / U256::from(10000)
}

fn percent_div(a: U256, bps: u64) -> U256 {
    let half_bps = bps / 2;
    (U256::from(half_bps) + (a * 10000)) / bps
}
