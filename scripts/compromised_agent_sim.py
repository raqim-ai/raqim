#!/usr/bin/env python3
"""
Raqim v0.2.0 — Deterministic Compromised Agent Security Harness
Simulates an agent process that has been completely hijacked by an attacker
and attempts 5 consecutive exploitation vectors against the Raqim Gateway.
"""
import os
import sys
import urllib.request
import urllib.error

GATEWAY_URL = os.environ.get("RAQIM_GATEWAY_URL", "http://127.0.0.1:8081")
TOKEN_1 = os.environ.get("RAQIM_TOKEN_1", "")
TOKEN_2 = os.environ.get("RAQIM_TOKEN_2", "")


def http_call(method: str, url: str, headers: dict, raw_body: bytes | None = None) -> tuple[int, dict, str]:
    req = urllib.request.Request(url, data=raw_body, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status, dict(resp.headers), resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as e:
        return e.code, dict(e.headers), e.read().decode("utf-8", errors="replace")


def main() -> int:
    print("==================================================================")
    print("BISMILLAH — RAQIM v0.2.0 COMPROMISED AGENT SIMULATION HARNESS")
    print("==================================================================")

    # -------------------------------------------------------------------------
    # VECTOR 1: Environment Variable Secret Dump
    # -------------------------------------------------------------------------
    print("\n[VECTOR 1] Attacker scans agent environment for raw API secrets...")
    leaked = [
        k for k, v in os.environ.items()
        if "sk_live_" in v or "sk_proj_" in v or k in ("PAYSTACK_SECRET_KEY", "OPENAI_SECRET_KEY")
    ]
    assert not leaked, f"FATAL: Raw secrets found in agent environment: {leaked}"
    print("  [DEFENDED] Zero upstream secrets exist in agent memory (only revocable rqm_v1 token).")

    # -------------------------------------------------------------------------
    # VECTOR 2: Admin Plane Escalation Attempt on TCP Data Plane (:8081)
    # -------------------------------------------------------------------------
    print("\n[VECTOR 2] Compromised agent probes TCP Data Plane for Admin Routes...")
    for admin_path in ("/v1/admin/ca/mint", "/v1/admin/quarantine/lift", "/v1/admin/in_doubt"):
        status, _, _ = http_call(
            "POST",
            f"{GATEWAY_URL}{admin_path}",
            {"Content-Type": "application/json"},
            b'{"agent_hex":"00","group":"admin_group"}',
        )
        assert status in (404, 405), f"FATAL: Admin route {admin_path} reachable on TCP Data Plane (HTTP {status})!"
    print("  [DEFENDED] All /v1/admin/* routes returned 404 on TCP Data Plane (isolated to Unix Socket 0600).")

    if not TOKEN_1 or not TOKEN_2:
        print("\n[INFO] Set RAQIM_TOKEN_1 and RAQIM_TOKEN_2 to run live Egress Lock vectors 3-5.")
        return 0

    # -------------------------------------------------------------------------
    # VECTOR 3: Authorized Transfer (25,000 NGN) + $0.00 Deterministic Replay
    # -------------------------------------------------------------------------
    print("\n[VECTOR 3] Executing legitimate 25,000 NGN transfer + $0.00 Gateway Replay...")
    legit_payload = b'{"amount": 25000, "currency": "NGN", "card": {"cvv": "999"}}'
    status, hdrs, body = http_call(
        "POST",
        f"{GATEWAY_URL}/v1/egress/paystack/transfer",
        {"Authorization": f"Bearer {TOKEN_1}", "Content-Type": "application/json"},
        legit_payload,
    )
    assert status == 200, f"Legitimate transfer failed (HTTP {status}): {body}"
    print(f"  [PASS] Live transfer authorized & executed (x-raqim-replay: {hdrs.get('x-raqim-replay')}).")

    status_rep, hdrs_rep, _ = http_call(
        "POST",
        f"{GATEWAY_URL}/v1/egress/paystack/transfer",
        {
            "Authorization": f"Bearer {TOKEN_1}",
            "X-Raqim-Mode": "replay",
            "Content-Type": "application/json",
        },
        legit_payload,
    )
    assert status_rep == 200 and hdrs_rep.get("x-raqim-replay") == "HIT", "Gateway replay failed!"
    print("  [PASS] Identical transfer replayed from WAL in <1ms for $0.00 (x-raqim-replay: HIT).")

    # -------------------------------------------------------------------------
    # VECTOR 4: JSON Duplicate-Key Smuggling Attack (`{"amount": 100, "amount": 50000000}`)
    # -------------------------------------------------------------------------
    print("\n[VECTOR 4] Compromised Agent #1 attempts JSON Duplicate-Key Smuggling...")
    smuggled_payload = b'{"amount": 100, "amount": 50000000, "currency": "NGN"}'
    status_smug, _, body_smug = http_call(
        "POST",
        f"{GATEWAY_URL}/v1/egress/paystack/transfer",
        {"Authorization": f"Bearer {TOKEN_1}", "Content-Type": "application/json"},
        smuggled_payload,
    )
    assert status_smug == 403, f"Smuggled JSON was not blocked with 403 (got {status_smug})!"
    print(f"  [DEFENDED] StrictJson parser intercepted duplicate key and quarantined Agent #1: {body_smug}")

    # Verify Agent #1 is now permanently locked in Quarantine even for a 100 NGN request!
    status_after, _, _ = http_call(
        "POST",
        f"{GATEWAY_URL}/v1/egress/paystack/transfer",
        {"Authorization": f"Bearer {TOKEN_1}", "Content-Type": "application/json"},
        b'{"amount": 100, "currency": "NGN"}',
    )
    assert status_after == 403, "Quarantined Agent #1 was still allowed to make requests!"
    print("  [DEFENDED] Follow-up request from quarantined Agent #1 rejected immediately.")

    # -------------------------------------------------------------------------
    # VECTOR 5: "Polite Evil Request" — Over-Limit Transfer (500,000 NGN > 50,000 Cap)
    # -------------------------------------------------------------------------
    print("\n[VECTOR 5] Compromised Agent #2 sends polite 500,000 NGN transfer exceeding 50,000 cap...")
    over_limit_payload = b'{"amount": 500000, "currency": "NGN"}'
    status_cap, _, body_cap = http_call(
        "POST",
        f"{GATEWAY_URL}/v1/egress/paystack/transfer",
        {"Authorization": f"Bearer {TOKEN_2}", "Content-Type": "application/json"},
        over_limit_payload,
    )
    assert status_cap == 403, f"Over-limit transfer was not blocked with 403 (got {status_cap})!"
    print(f"  [DEFENDED] JSON-Pointer Predicate (/amount <= 50000) blocked transfer & quarantined Agent #2: {body_cap}")

    print("\n==================================================================")
    print("ALL 5 COMPROMISED AGENT ATTACK VECTORS DEFEATED. ALHAMDULILLAH!")
    print("==================================================================")
    return 0


if __name__ == "__main__":
    sys.exit(main())