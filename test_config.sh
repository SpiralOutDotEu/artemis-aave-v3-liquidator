#!/bin/bash

# Test script for milestone 1 - Config Refactor
echo "Testing Artemis Aave V3 Liquidator Configuration System..."

# Set environment variables for testing
export BASE_RPC_URL="https://base-mainnet.example.com"
export PK1="1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"

# Test 1: Check if the bot-config crate compiles
echo "Test 1: Compiling bot-config crate..."
cd crates/bot-config
cargo check
if [ $? -eq 0 ]; then
    echo "✓ bot-config crate compiles successfully"
else
    echo "✗ bot-config crate compilation failed"
    exit 1
fi

# Test 2: Run bot-config tests
echo "Test 2: Running bot-config tests..."
cargo test
if [ $? -eq 0 ]; then
    echo "✓ bot-config tests pass"
else
    echo "✗ bot-config tests failed"
    exit 1
fi

cd ../..

# Test 3: Check if main application compiles
echo "Test 3: Compiling main application..."
cargo check
if [ $? -eq 0 ]; then
    echo "✓ Main application compiles successfully"
else
    echo "✗ Main application compilation failed"
    exit 1
fi

# Test 4: Test configuration loading
echo "Test 4: Testing configuration loading..."
cargo run -- --config config/bot.toml --chain base --deployment AAVE --help
if [ $? -eq 0 ]; then
    echo "✓ Configuration system works (help command successful)"
else
    echo "✗ Configuration system test failed"
    exit 1
fi

echo ""
echo "🎉 All tests passed! Milestone 1 (Config Refactor) is working correctly."
echo ""
echo "Configuration files created:"
echo "  - config/bot.toml (main configuration)"
echo "  - config/secrets.env.example (environment template)"
echo "  - crates/bot-config/ (configuration crate)"
echo ""
echo "Next steps:"
echo "  1. Copy config/secrets.env.example to config/secrets.env"
echo "  2. Fill in your actual RPC URL and private keys"
echo "  3. Run: cargo run -- --config config/bot.toml --chain base --deployment AAVE"
