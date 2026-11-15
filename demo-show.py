#!/usr/bin/env python3
"""
Presentation wrapper for `demo.py` with self-paced `[Enter]` beats enabled.
"""

import asyncio
import os

os.environ.setdefault("DEMO_PAUSE", "1")

from demo import main  # noqa: E402
from demo_utils import stop_demo_daemon, stop_mock_upstream_server  # noqa: E402


if __name__ == "__main__":
    try:
        asyncio.run(main())
    finally:
        stop_demo_daemon()
        stop_mock_upstream_server()
    os._exit(0)
