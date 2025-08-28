use anyhow::Result;
use clap::Parser;
use std::str::FromStr;
use std::path::PathBuf;

use artemis_core::engine::Engine;
use artemis_core::types::{CollectorMap, ExecutorMap};
use collectors::time_collector::TimeCollector;
use ethers::{
    prelude::MiddlewareBuilder,
    providers::{Http, Provider},
    signers::{LocalWallet, Signer},
};
use executors::protect_executor::ProtectExecutor;
use std::sync::Arc;
use strategies::{
    aave_strategy::{AaveStrategy, Deployment},
    types::{Action, Config, Event},
};
use tracing::{info, Level};
use tracing_subscriber::{filter, prelude::*};
use std::fs;
use std::path::Path;
use bot_config::{Mode, CollectorType};

pub mod collectors;
pub mod executors;
pub mod strategies;
pub mod addresses;

/// Default chain ID for Base network
pub const CHAIN_ID: u64 = 8453;

/// Command-line interface arguments for the liquidator bot
#[derive(Parser, Debug)]
pub struct Args {
    /// Path to the bot configuration file (TOML format)
    #[arg(long, default_value = "config/bot.toml")]
    pub config: PathBuf,

    /// Target blockchain network (e.g., "base", "arbitrum")
    #[arg(long)]
    pub chain: Option<String>,

    /// Target deployment/protocol (e.g., "AAVE", "SEAMLESS")
    #[arg(long)]
    pub deployment: Option<String>,

    /// Override the operation mode from config (live/simulate)
    #[arg(long)]
    pub mode: Option<Mode>,

    /// Override the event collection strategy from config (time/block/pending)
    #[arg(long)]
    pub collector: Option<CollectorType>,
}

/// Sets up comprehensive logging system with both console and file output
/// 
/// Creates necessary directories and configures:
/// - Console logging with enhanced formatting
/// - File logging with timestamps and full context
/// - Structured logging with appropriate log levels
/// 
/// # Arguments
/// * `config` - Bot configuration containing data directory and log settings
/// 
/// # Returns
/// * `Result<()>` - Success or error from logging setup
fn setup_advanced_logging(config: &bot_config::BotConfig) -> Result<()> {
    // Create main data directory if it doesn't exist
    let data_dir = Path::new(&config.app.data_dir);
    if !data_dir.exists() {
        fs::create_dir_all(data_dir)?;
        println!("Created data directory: {}", data_dir.display());
    }
    
    // Create logs subdirectory for organized log storage
    let logs_dir = data_dir.join("logs");
    if !logs_dir.exists() {
        fs::create_dir_all(&logs_dir)?;
        println!("Created logs directory: {}", logs_dir.display());
    }
    
    // Generate timestamped log filename for easy identification
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let log_file = logs_dir.join(format!("bot_{}.log", timestamp));
    
    // Configure console logging layer with enhanced visibility
    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(true)        // Show module targets
        .with_thread_ids(true)    // Show thread identifiers
        .with_thread_names(true); // Show thread names
    
    // Create log file and configure file logging layer
    let log_file_path = logs_dir.join(format!("bot_{}.log", timestamp));
    let file = std::fs::File::create(&log_file_path)?;
    
    // Configure file logging with comprehensive context
    let file_layer = tracing_subscriber::fmt::layer()
        .with_target(true)        // Include module targets
        .with_thread_ids(true)    // Include thread IDs
        .with_thread_names(true)  // Include thread names
        .with_file(true)          // Include source file names
        .with_line_number(true)   // Include line numbers
        .with_ansi(false)         // Disable ANSI colors for file output
        .with_writer(file);
    
    // Configure log filtering to control verbosity
    let filter = filter::Targets::new()
        .with_target("artemis_core", Level::INFO)
        .with_target("aave_v3_liquidator", Level::INFO);
    
    // Initialize the logging registry with all configured layers
    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .with(filter)
        .init();
    
    println!("Advanced logging initialized. Logs stored in: {}", logs_dir.display());
    println!("Current log file: {}", log_file.display());
    
    Ok(())
}

/// Main entry point for the Aave V3 liquidator bot
/// 
/// The bot follows this startup sequence:
/// 1. Parse command-line arguments
/// 2. Load and validate configuration
/// 3. Set up logging and blockchain connection
/// 4. Initialize the trading engine with strategy and executor
/// 5. Start monitoring for liquidation opportunities
#[tokio::main]
async fn main() -> Result<()> {
    // Parse command-line arguments first
    let args = Args::parse();
    println!("{:?}", args);
    
    // Load configuration from TOML file with environment variable expansion
    let mut config = bot_config::load_from_file(&args.config)?;
    
    // Initialize comprehensive logging system
    setup_advanced_logging(&config)?;
    
    // Apply command-line overrides to configuration
    if let Some(mode) = args.mode {
        config.app.mode = mode;
    }
    if let Some(collector) = args.collector {
        config.app.collector = collector;
    }
    
    // Determine target chain and deployment from args or defaults
    let chain_name = args.chain.unwrap_or_else(|| "base".to_string());
    let deployment_name = args.deployment.unwrap_or_else(|| "AAVE".to_string());
    
    // Retrieve chain configuration
    let chain = config.chains.get(&chain_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown chain: {}", chain_name))?;
    
    // Retrieve deployment configuration for the selected chain
    let deployment = config.deployments.get(&chain_name)
        .and_then(|m| m.get(&deployment_name))
        .ok_or_else(|| anyhow::anyhow!("Unknown deployment: {}.{}", chain_name, deployment_name))?;
    
    // Log configuration details for transparency
    info!("Using chain: {} (ID: {})", chain_name, chain.chain_id);
    info!("Using deployment: {} (Pool: {})", deployment_name, deployment.pool);
    
    // Validate configuration integrity before proceeding
    bot_config::validate_config(&config)?;
    
    // Initialize blockchain connection with RPC provider
    let rpc = Http::from_str(&chain.rpc_url)?;
    let provider = Provider::new(rpc);
    
    // Set up wallet for transaction signing
    // TODO: Support multiple wallets and wallet rotation
    let wallet_private_key = &config.execution.wallets[0];
    let wallet: LocalWallet = wallet_private_key.parse::<LocalWallet>()?
        .with_chain_id(chain.chain_id);
    let address = wallet.address();
    
    // Create provider with nonce management and signing capabilities
    let provider = Arc::new(provider.nonce_manager(address).with_signer(wallet.clone()));
    
    // Extract contract addresses from deployment configuration
    let addresses = addresses::Addresses::from(deployment);
    
    // Initialize the trading engine
    let mut engine: Engine<Event, Action> = Engine::default();
    
    // Configure event collector based on user preference
    let collector = match config.app.collector {
        CollectorType::Time => {
            // Time-based polling for consistent intervals
            let time_collector = Box::new(TimeCollector::new(config.app.tick_ms / 1000));
            CollectorMap::new(time_collector, Event::NewTick)
        }
        CollectorType::Block => {
            // TODO: Implement block-based collector in Milestone 4
            // For now, fall back to time-based collection
            let time_collector = Box::new(TimeCollector::new(config.app.tick_ms / 1000));
            CollectorMap::new(time_collector, Event::NewTick)
        }
        CollectorType::Pending => {
            // TODO: Implement pending transaction collector in Milestone 4
            // For now, fall back to time-based collection
            let time_collector = Box::new(TimeCollector::new(config.app.tick_ms / 1000));
            CollectorMap::new(time_collector, Event::NewTick)
        }
    };
    engine.add_collector(Box::new(collector));
    
    // Create strategy configuration from bot settings
    let strategy_config = Config {
        bid_percentage: config.execution.tip_fraction_of_profit_bps,
        chain_id: chain.chain_id,
    };
    
    // Convert deployment name string to enum for type safety
    let deployment_enum = match deployment_name.as_str() {
        "AAVE" => Deployment::AAVE,
        "SEAMLESS" => Deployment::SEAMLESS,
        _ => return Err(anyhow::anyhow!("Invalid deployment: {}", deployment_name)),
    };
    
    // Initialize the Aave liquidation strategy
    let strategy = AaveStrategy::new(
        Arc::new(provider.clone()),
        strategy_config,
        deployment_enum,
        addresses,
        chain.start_block, // Use configured start block for efficient scanning
        config.app.data_dir.clone(), // Pass data directory for cache and logs
    );
    engine.add_strategy(Box::new(strategy));
    
    // Set up transaction executor with protection mechanisms
    let executor = Box::new(ProtectExecutor::new(provider.clone(), provider.clone()));
    let executor = ExecutorMap::new(executor, |action| match action {
        Action::SubmitTx(tx) => Some(tx),
    });
    engine.add_executor(Box::new(executor));
    
    // Start the trading engine and begin monitoring
    if let Ok(mut set) = engine.run().await {
        info!("🚀 Engine started successfully! Bot is now running and monitoring for liquidation opportunities.");
        info!("⏰ Tick interval: {} seconds (configurable in bot.toml)", config.app.tick_ms / 1000);
        info!("📱 Press Ctrl+C to stop the bot.");
        info!("💡 Bot will show progress every tick and heartbeat every 5 ticks");

        let mut heartbeat_counter = 0;
        let tick_interval_secs = config.app.tick_ms / 1000;
        
        // Start background task to provide user feedback during waiting periods
        let tick_interval_secs = tick_interval_secs.clone();
        let waiting_handle = tokio::spawn(async move {
            let mut tick_count = 0;
            loop {
                tick_count += 1;
                info!("⏳ Waiting {} seconds until next tick #{} to respect RPC rate limits...", tick_interval_secs, tick_count);
                
                // Wait for the configured interval without blocking the main loop
                tokio::time::sleep(std::time::Duration::from_secs(tick_interval_secs)).await;
                info!("✅ Tick #{} ready! Processing blockchain data...", tick_count);
            }
        });
        
        // Main event processing loop
        while let Some(res) = set.join_next().await {
            heartbeat_counter += 1;
            // Show heartbeat every 10 events to confirm bot is still active
            if heartbeat_counter % 10 == 0 {
                info!("💓 Bot heartbeat: Still running and monitoring (processed {} events)", heartbeat_counter);
            }
            
            info!("📊 Event result: {:?}", res);
        }
        
        // Clean up background task when main loop ends
        waiting_handle.abort();
    }
    Ok(())
}
