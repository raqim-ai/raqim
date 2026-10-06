#!/usr/bin/env bash
# ==============================================================================
# Raqim Interactive Showcase Runner
# ==============================================================================
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Source environment variables if .env exists
if [ -f "./raqim-py/.env" ]; then
    set -a
    source ./raqim-py/.env
    set +a
elif [ -f "./.env" ]; then
    set -a
    source ./.env
    set +a
fi

# Check if Python virtual environment exists in raqim-py
if [ -f "./raqim-py/.venv/bin/python3" ]; then
    PYTHON_BIN="./raqim-py/.venv/bin/python3"
elif command -v python3 >/dev/null 2>&1; then
    PYTHON_BIN="python3"
else
    echo "Error: python3 is required to run the demo."
    exit 1
fi

echo "=================================================================="
echo "Starting Raqim Sovereign Execution-Integrity Demonstration..."
echo "Using Python: $PYTHON_BIN"
echo "=================================================================="

# Execute demo with unbuffered output
exec "$PYTHON_BIN" -u demo.py "$@"
