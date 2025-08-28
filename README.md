# Artemis Aave V3 Liquidator

A liquidator bot for Aave V3 with a flexible, configuration-driven architecture and advanced monitoring capabilities.

## 🚀 Quick Start

### 0. Environment Variables Required

**You need exactly 2 environment variables:**

- `BASE_RPC_URL` - Your Base network RPC endpoint
- `PK1` - Your wallet private key (hex format, no 0x prefix)

**Setup method (choose one):**

1. **Recommended**: Copy `config/secrets.env.example` to `config/secrets.env` and edit
2. **Alternative**: Set shell environment variables

### 1. Configuration Setup

```bash
# Copy the environment template
cp config/secrets.env.example config/secrets.env

# Edit with your actual values
nano config/secrets.env
```

**Required Environment Variables:**

The bot requires exactly **2 environment variables**:

- `BASE_RPC_URL` - Your Base network RPC endpoint
- `PK1` - Your wallet private key (hex format, no 0x prefix)

**Important**: The bot automatically loads these from `config/secrets.env` first, then falls back to shell environment variables. `secrets.env` takes priority for security.

### 2. Run the Bot

```bash
# Use default configuration (Base + AAVE)
cargo run -- --config config/bot.toml

# Specify chain and deployment
cargo run -- --config config/bot.toml --chain base --deployment AAVE

# Run in simulation mode (no transactions)
cargo run -- --config config/bot.toml --mode simulate

# Use block-based collector for faster response
cargo run -- --config config/bot.toml --collector block
```

## ✨ New Features (Milestone 1)

### 🔧 Configuration System Overhaul

- **TOML-based configuration** replacing hardcoded values
- **Multi-chain support** with easy addition of new networks
- **Multi-deployment support** for different Aave V3 forks
- **Environment variable expansion** for secure secret management
- **Configuration validation** at startup

### 📊 Enhanced Monitoring & Logging

- **Comprehensive logging system** with both console and file output
- **Progress indicators** showing real-time bot activity
- **Heartbeat monitoring** to confirm bot is alive and working
- **Detailed event processing** with emoji-based status updates
- **Timestamped log files** for easy debugging and analysis

### 🚀 Performance Improvements

- **Efficient state management** with persistent caching
- **Chunked log processing** to respect RPC rate limits
- **Timeout handling** for reliable RPC interactions
- **Background task management** for non-blocking operations

### 🏗️ Architecture Improvements

- **Modular design** with separate configuration crate
- **Type-safe configuration** with compile-time validation
- **Clean separation** of concerns between components
- **Extensible structure** for future enhancements

## ⚙️ Configuration

### Configuration File Structure

The bot uses `config/bot.toml` for all runtime parameters. Environment variables are automatically expanded from `${VARIABLE_NAME}` syntax.

**Environment Variable Priority:**

1. `config/secrets.env` file (highest priority - recommended)
2. Shell environment variables (fallback)

```toml
[app]
mode = "live"             # "live" | "simulate"
log_level = "info"
data_dir = ".botdata"     # Directory for cache and logs
tick_ms = 300000          # Polling interval (ms)
collector = "time"        # "time" | "block" | "pending"

[chains.base]
chain_id = 8453
rpc_url = "${BASE_RPC_URL}"         # Environment variable: BASE_RPC_URL
explorer.base_url = "https://basescan.org"
start_block = 2963358               # Efficient scanning from specific block

[deployments.base.AAVE]
pool = "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5"
pool_data_provider = "0x2d8A3C5677189723C4cB8873CfC9C8976FDF38Ac"
oracle = "0x2Cc0Fc26eD4563A5ce5e8bdcfe1A2878676Ae156"
l2_encoder = "0x39e97c588B2907Fb67F44fea256Ae3BA064207C5"
uniswap_v3.factory = "0x33128a8fC17869897dcE68Ed026d694621f6FDfD"
weth = "0x4200000000000000000000000000000000000006"

[strategy]
max_candidates = 50
close_factor_bps = 5000         # 50% liquidation factor
min_profit_usd = 10.0          # Minimum profit threshold
slippage_bps = 50              # 0.50% slippage tolerance
prefer_atoken = false

[execution]
wallets = ["${PK1}"]           # Environment variable: PK1
submitters = ["public"]
tip_fraction_of_profit_bps = 500  # 5% of profit as gas tip
```

### Environment Variables

The bot automatically loads environment variables from `config/secrets.env` with fallback to shell environment variables. **`secrets.env` takes priority over shell environment variables for security.**

**Only 2 variables are required:**

- `BASE_RPC_URL` - Your Base network RPC endpoint
- `PK1` - Your wallet private key (hex format, no 0x prefix)

**Option 1: Use secrets.env (recommended)**

```bash
# The bot automatically loads from config/secrets.env
# Just run the bot - no additional setup needed
cargo run -- --config config/bot.toml
```

**Option 2: Set in shell (fallback)**

```bash
# Set environment variables in your shell
export BASE_RPC_URL="https://your-base-rpc-endpoint.com"
export PK1="1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"

# Then run the bot
cargo run -- --config config/bot.toml
```

**Note**: The `config/secrets.env` file is ignored by git for security. Copy `config/secrets.env.example` and fill in your actual values.

**Summary**: You only need to set 2 environment variables (`BASE_RPC_URL` and `PK1`) in either `config/secrets.env` or your shell. The `secrets.env` method is recommended for security.

### Adding New Chains/Deployments

To add support for a new chain (e.g., Arbitrum):

```toml
[chains.arbitrum]
chain_id = 42161
rpc_url = "${ARB_RPC_URL}"
explorer.base_url = "https://arbiscan.io"
start_block = 172000000

[deployments.arbitrum.AAVE]
pool = "0x794a61358D6845594F94dc1DB02A252b5b4814aD"
pool_data_provider = "0x69FA688f1Dc47d4B5d8029D5a35FB7fD5483e9ae"
oracle = "0x54586bE62E3c8DEaaf53C2B0f2d4C5d2B0f2d4C5"
# ... other addresses
```

Then set the environment variable:

```bash
export ARB_RPC_URL="https://your-arbitrum-rpc.com"
```

## 📊 Monitoring & Logging

### Real-Time Status Updates

The bot provides comprehensive monitoring with:

- **🔄 Processing indicators** for each tick
- **💓 Heartbeat messages** every 5 ticks
- **📊 Progress tracking** for blockchain operations
- **✅ Success confirmations** for completed operations
- **❌ Error reporting** with detailed context

### Log File Management

```bash
# Logs are automatically stored in:
.botdata/logs/bot_YYYYMMDD_HHMMSS.log

# Each run creates a new timestamped log file
# Logs include full context: file names, line numbers, thread IDs
```

### Performance Monitoring

```bash
# Monitor bot activity in real-time
tail -f .botdata/logs/bot_*.log

# Check bot status
grep "💓 Bot heartbeat" .botdata/logs/bot_*.log

# Monitor liquidation opportunities
grep "💰 Found opportunity" .botdata/logs/bot_*.log
```

## 🧪 Testing

### 1. Test Configuration Loading

```bash
# Test help command
cargo run -- --help

# Test configuration parsing
cargo run -- --config config/bot.toml --chain base --deployment AAVE --help
```

**Note**: The bot automatically loads from `config/secrets.env`. If you get "Environment variable not found" errors, check that your `config/secrets.env` file contains the required variables:

```bash
# Check your secrets.env file (should contain BASE_RPC_URL and PK1)
cat config/secrets.env

# Then test
cargo run -- --config config/bot.toml --chain base --deployment AAVE --help
```

### 2. Test Bot-Config Crate

```bash
# Test the configuration crate
cd crates/bot-config
cargo test
cd ../..
```

### 3. Test Main Application

```bash
# Check compilation
cargo check

# Test with invalid config (should fail)
cargo run -- --config nonexistent.toml
```

### 4. Run Test Script

```bash
# Make executable and run
chmod +x test_config.sh
./test_config.sh
```

## 🔧 Development

### Project Structure

```
├── crates/
│   └── bot-config/           # Configuration management
│       ├── src/
│       │   ├── types.rs      # Configuration structures
│       │   ├── load.rs       # File loading & env expansion
│       │   └── validate.rs   # Configuration validation
│       └── Cargo.toml
├── config/
│   ├── bot.toml              # Main configuration
│   └── secrets.env.example   # Environment template
├── src/
│   ├── addresses.rs          # Address extraction & management
│   ├── main.rs               # Main application with enhanced logging
│   └── strategies/           # Trading strategies with improved monitoring
└── test_config.sh            # Test script
```

### Adding New Configuration Options

1. **Add to types.rs:**

```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NewConfig {
    pub option: String,
    pub value: u64,
}
```

2. **Add to bot.toml:**

```toml
[new_section]
option = "default_value"
value = 100
```

3. **Update validation in validate.rs:**

```rust
fn validate_new_config(config: &NewConfig) -> Result<()> {
    if config.value == 0 {
        return Err(anyhow!("value cannot be zero"));
    }
    Ok(())
}
```

### Configuration Validation

The bot validates configuration at startup:

- ✅ Address format validation
- ✅ Chain ID validation
- ✅ Business rule validation
- ✅ Environment variable presence
- ✅ File format validation

## 🚨 Troubleshooting

### Common Issues

**"Environment variable not found"**

```bash
# Check if variables are set in secrets.env (recommended)
cat config/secrets.env

# Or check shell environment variables (fallback)
echo $BASE_RPC_URL
echo $PK1

# Set them if missing (use secrets.env method)
cp config/secrets.env.example config/secrets.env
nano config/secrets.env  # Edit with your actual values
```

**"Invalid configuration"**

```bash
# Validate TOML syntax
cargo run -- --config config/bot.toml --help

# Check for missing required fields
grep -n "pool\|oracle\|weth" config/bot.toml
```

**"Address validation failed"**

```bash
# Check address format (must be valid hex)
grep "0x" config/bot.toml

# Ensure addresses are 40 characters (20 bytes)
# Example: 0xA238Dd80C259a72e81d7e4664a9801593F98d1c5
```

### Debug Mode

```bash
# Enable debug logging
RUST_LOG=debug cargo run -- --config config/bot.toml

# Check configuration loading
RUST_LOG=info cargo run -- --config config/bot.toml --chain base --deployment AAVE
```

### Log Analysis

```bash
# Analyze recent bot activity
./analyze_logs.sh

# Monitor logs in real-time
./monitor_logs.sh

# View logs with Python viewer
./start_log_viewer.sh
```

## 📚 CLI Reference

### Global Options

```bash
--config <PATH>          # Configuration file path [default: config/bot.toml]
--chain <NAME>           # Chain name (e.g., "base", "arbitrum")
--deployment <NAME>      # Deployment name (e.g., "AAVE", "SEAMLESS")
--mode <MODE>            # Operation mode: "live" | "simulate"
--collector <TYPE>       # Collector type: "time" | "block" | "pending"
-h, --help               # Show help
```

### Examples

```bash
# Live trading on Base AAVE
cargo run -- --config config/bot.toml --chain base --deployment AAVE

# Simulation mode on Base SEAMLESS
cargo run -- --config config/bot.toml --chain base --deployment SEAMLESS --mode simulate

# Block-based collector for faster response
cargo run -- --config config/bot.toml --collector block

# Custom configuration file
cargo run -- --config my_config.toml --chain base --deployment AAVE
```

## 🔒 Security Notes

- **Never commit** `config/secrets.env` to version control
- **Use strong RPC endpoints** with authentication
- **Rotate private keys** regularly
- **Monitor gas usage** and profit thresholds
- **Test in simulation mode** before live trading

## 📈 Next Steps

This bot is part of a larger upgrade roadmap:

- ✅ **Milestone 1: Config Refactor** (COMPLETED)
  - Configuration system overhaul
  - Enhanced logging and monitoring
  - Multi-chain/deployment support
  - Performance improvements
- 🚧 **Milestone 2: Simulation Mode** (NEXT)
- 📋 **Milestone 3: Liquidator Contract Refactor**
- 📋 **Milestone 4: Per-block Reactivity**

For detailed upgrade information, see [upgrade_plan.md](upgrade_plan.md).

## 🆕 What's New in Milestone 1

### Configuration Management

- **TOML-based configs** instead of hardcoded values
- **Environment variable expansion** for secure secrets
- **Multi-chain support** with easy network addition
- **Configuration validation** at startup

### Enhanced Monitoring

- **Real-time progress indicators** with emojis
- **Comprehensive logging** to both console and files
- **Heartbeat monitoring** to confirm bot health
- **Detailed event processing** with status updates

### Performance & Reliability

- **Persistent state caching** for efficient restarts
- **Chunked log processing** respecting RPC limits
- **Timeout handling** for reliable operations
- **Background task management** for non-blocking ops

### Developer Experience

- **Modular architecture** with separate config crate
- **Type-safe configuration** with compile-time checks
- **Clean separation** of concerns
- **Extensible structure** for future enhancements
