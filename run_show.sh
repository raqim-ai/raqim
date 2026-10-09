#!/usr/bin/env bash
# ==============================================================================
# Raqim Interactive Showcase Presentation Runner (Self-Paced with Enter Beats)
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
    if ! "$PYTHON_BIN" -c "import httpx, blake3, nacl" >/dev/null 2>&1; then
        echo "Error: Required Python dependencies (httpx, blake3, pynacl) are missing in $PYTHON_BIN."
        echo "Please set up the virtual environment: cd raqim-py && python3 -m venv .venv && source .venv/bin/activate && pip install -e ."
        exit 1
    fi
else
    echo "Error: python3 is required to run the demo."
    exit 1
fi

echo "=================================================================="
echo "Starting Raqim Presentation Mode (Self-Paced Walkthrough)..."
echo "Using Python: $PYTHON_BIN"
echo "=================================================================="

# Execute demo-show with unbuffered output
exec "$PYTHON_BIN" -u demo-show.py "$@"

