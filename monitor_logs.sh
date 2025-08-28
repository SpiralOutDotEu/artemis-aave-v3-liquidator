#!/bin/bash

# Real-time Log Monitor for Artemis Aave V3 Liquidator
# Usage: ./monitor_logs.sh [log_file] [filter]

LOG_FILE="${1:-.botdata/logs/bot_*.log}"
FILTER="${2:-}"

echo "📺 Real-time Log Monitor - Artemis Aave V3 Liquidator"
echo "======================================================"
echo "Monitoring: $LOG_FILE"
if [ ! -z "$FILTER" ]; then
    echo "Filter: $FILTER"
fi
echo "Press Ctrl+C to stop monitoring"
echo ""

# Function to colorize log levels
colorize_logs() {
    sed -e 's/\(ERROR\)/\x1b[31m\1\x1b[0m/g' \
        -e 's/\(WARN\)/\x1b[33m\1\x1b[0m/g' \
        -e 's/\(INFO\)/\x1b[32m\1\x1b[0m/g' \
        -e 's/\(DEBUG\)/\x1b[36m\1\x1b[0m/g' \
        -e 's/\(ThreadId(01)\)/\x1b[1;34m\1\x1b[0m/g' \
        -e 's/\(main\)/\x1b[1;35m\1\x1b[0m/g' \
        -e 's/\(tokio-runtime-worker\)/\x1b[1;36m\1\x1b[0m/g'
}

# Function to filter logs
filter_logs() {
    if [ ! -z "$FILTER" ]; then
        grep "$FILTER"
    else
        cat
    fi
}

# Monitor logs in real-time
if [ ! -z "$FILTER" ]; then
    echo "🔍 Filtering logs for: $FILTER"
    tail -f $LOG_FILE | filter_logs | colorize_logs
else
    echo "📺 Showing all logs (use './monitor_logs.sh [file] [filter]' to filter)"
    tail -f $LOG_FILE | colorize_logs
fi



