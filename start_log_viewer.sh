#!/bin/bash

# Artemis Log Viewer Startup Script
echo "🔍 Starting Artemis Log Viewer..."
echo ""

# Check if Python3 is available
if ! command -v python3 &> /dev/null; then
    echo "❌ Python3 is not installed. Please install Python3 first."
    exit 1
fi

# Check if Flask is available
if ! python3 -c "import flask" &> /dev/null; then
    echo "📦 Installing Flask..."
    pip3 install flask
fi

# Check if logs directory exists
if [ ! -d ".botdata/logs" ]; then
    echo "❌ No logs directory found. Please run the bot first to generate logs."
    exit 1
fi

# Count log files
LOG_COUNT=$(ls .botdata/logs/*.log 2>/dev/null | wc -l)
if [ $LOG_COUNT -eq 0 ]; then
    echo "⚠️  No log files found in .botdata/logs/"
    echo "   Run the bot first to generate some logs."
    echo ""
fi

echo "✅ Starting log viewer..."
echo "📱 Open your browser and go to: http://localhost:5000"
echo "📁 Logs directory: .botdata/logs/"
echo "⏹️  Press Ctrl+C to stop the viewer"
echo ""

# Start the log viewer
python3 log_viewer.py



