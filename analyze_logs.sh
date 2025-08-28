#!/bin/bash

# Log Analysis Script for Artemis Aave V3 Liquidator
# Usage: ./analyze_logs.sh [log_file_pattern]

LOG_PATTERN="${1:-.botdata/logs/bot_*.log}"

echo "🔍 Artemis Aave V3 Liquidator - Log Analysis"
echo "=============================================="
echo "Log files: $LOG_PATTERN"
echo ""

# Check if log files exist
if ! ls $LOG_PATTERN >/dev/null 2>&1; then
    echo "❌ No log files found matching pattern: $LOG_PATTERN"
    exit 1
fi

echo "📊 Basic Statistics"
echo "-------------------"

# Count total log entries
TOTAL_LOGS=$(wc -l $LOG_PATTERN | awk '{sum += $1} END {print sum}')
echo "Total log entries: $TOTAL_LOGS"

# Count log files
LOG_COUNT=$(ls $LOG_PATTERN | wc -l)
echo "Log files: $LOG_COUNT"

echo ""
echo "📈 Log Level Distribution"
echo "------------------------"

# Count by log level
echo "Log levels found:"
grep -h "INFO\|ERROR\|WARN\|DEBUG" $LOG_PATTERN | awk '{print $3}' | sort | uniq -c | sort -nr

echo ""
echo "🧵 Thread Analysis"
echo "------------------"

# Count by thread
echo "Threads found:"
grep -h "ThreadId" $LOG_PATTERN | awk '{print $4}' | sort | uniq -c | sort -nr

echo ""
echo "⏰ Time Analysis"
echo "----------------"

# Show time range
echo "Time range:"
FIRST_LOG=$(grep -h "^2025-" $LOG_PATTERN | head -1 | awk '{print $1}')
LAST_LOG=$(grep -h "^2025-" $LOG_PATTERN | tail -1 | awk '{print $1}')
echo "First log: $FIRST_LOG"
echo "Last log:  $LAST_LOG"

echo ""
echo "🔍 Error Analysis"
echo "-----------------"

# Show all errors
ERROR_COUNT=$(grep -h "ERROR" $LOG_PATTERN | wc -l)
echo "Total errors: $ERROR_COUNT"

if [ $ERROR_COUNT -gt 0 ]; then
    echo "Error details:"
    grep -h "ERROR" $LOG_PATTERN | head -5
    if [ $ERROR_COUNT -gt 5 ]; then
        echo "... and $((ERROR_COUNT - 5)) more errors"
    fi
fi

echo ""
echo "📝 Recent Activity"
echo "------------------"

# Show last 10 log entries
echo "Last 10 log entries:"
tail -10 $LOG_PATTERN | while read line; do
    echo "  $line"
done

echo ""
echo "🎯 Key Metrics"
echo "--------------"

# Count specific events
BORROW_LOGS=$(grep -h "borrow logs" $LOG_PATTERN | wc -l)
SUPPLY_LOGS=$(grep -h "supply logs" $LOG_PATTERN | wc -l)
LIQUIDATION_SCANS=$(grep -h "Scanning for liquidation" $LOG_PATTERN | wc -l)
STATE_UPDATES=$(grep -h "State update complete" $LOG_PATTERN | wc -l)

echo "Borrow log fetches: $BORROW_LOGS"
echo "Supply log fetches: $SUPPLY_LOGS"
echo "Liquidation scans: $LIQUIDATION_SCANS"
echo "State updates: $STATE_UPDATES"

echo ""
echo "✅ Analysis complete!"
echo ""
echo "💡 Tips:"
echo "  - Use 'grep \"ERROR\" $LOG_PATTERN' to find errors"
echo "  - Use 'grep \"ThreadId(01)\" $LOG_PATTERN' to follow main thread"
echo "  - Use 'tail -f $LOG_PATTERN' to watch logs in real-time"
echo "  - Use 'awk' for advanced field extraction"



