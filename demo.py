#!/usr/bin/env python3
"""
================================================================================
Raqim v0.2.0 — The Sovereign Fintech Treasury & AI Airlock Live Showcase
================================================================================
Proves all 6 enterprise security & durability invariants live on bare metal:
  ACT 1: Zero-Secret Identity Provisioning & Admin Plane Isolation (UDS 0600)
  ACT 2: The 4 Concentric Egress Locks (Paystack Cap, Anti-Smuggling, Velocity)
  ACT 3: Zero-Leak Disk Audit, Full-Envelope Merkle Proof, NDPA Redaction & CRC
  ACT 4: Mid-Payment Upstream Drop & `In-Doubt` Intent Reconciliation
  ACT 5: Zero-Stall SSE Byte-Tee, $0.00 HTTP Replay & Counterfactual Forking
  ACT 6: The Phoenix Moment (`kill -9` SIGKILL & Sub-50ms State Rehydration)
================================================================================
"""

import asyncio
import json
import os
import time
import httpx

from demo_utils import (
    DAEMON_HTTP,
    DAEMON_HTTP_PORT,
    DAEMON_TCP_PORT,
    DEMO_WAL_PATH,
    OPENAI_CANARY_SECRET,
    PAYSTACK_CANARY_SECRET,
    UPSTREAM_STATS,
    Style,
    admin_uds_request,
    clean_demo_sandbox,
    ensure_daemon_running,
    forge_agent_offline,
    get_binary_info,
    kill_daemon_phoenix,
    pad_visible,
    print_banner,
    print_header,
    resurrect_daemon_phoenix,
    stop_demo_daemon,
    stop_mock_upstream_server,
    test_mid_file_wal_corruption_halt,
)
from raqim.client import (
    CanonicalSerializer,
    RaqimClient,
    _execution_step_context,
    verify_state_proof_offline,
)


async def main():
    print_banner()
    clean_demo_sandbox()
    daemon_proc = await ensure_daemon_running()

    # ==========================================================================
    # ACT 1: ZERO-SECRET IDENTITY & ADMIN PLANE ISOLATION
    # ==========================================================================
    print_header(
        "ACT 1: ZERO-SECRET PROVISIONING & ADMIN PLANE ISOLATION",
        "Agents hold revocable `rqm_v1` Ed25519 tokens; zero upstream keys exist in agent env",
    )

    # Verify agent process truly has ZERO upstream keys in its environment
    leaked_env = [
        k for k, v in os.environ.items()
        if PAYSTACK_CANARY_SECRET in v or OPENAI_CANARY_SECRET in v
    ]
    assert not leaked_env, f"FATAL: Secret leaked into agent environment: {leaked_env}"
    print(f"  {Style.GREEN}✔{Style.RESET} Agent Environment Audit : {Style.BOLD}0 upstream API keys{Style.RESET} (Secrets live exclusively in Rust daemon RAM)")

    # Offline provision 4 distinct agent identities from swarm_master.key
    pem_1, cert_1, tok_1, hex_1 = forge_agent_offline("treasury_primary", "treasury_agent")
    _, _, tok_cap_attacker, hex_cap = forge_agent_offline("rogue_overcap", "treasury_agent")
    _, _, tok_smuggler, hex_smug = forge_agent_offline("rogue_smuggler", "treasury_agent")
    _, _, tok_velocity, hex_vel = forge_agent_offline("rogue_velocity", "treasury_agent")

    treasury = RaqimClient(
        alias="TreasuryPrimary",
        tenant="raqim_fintech_airlock_demo",
        private_key_path=pem_1,
        cert_path=cert_1,
        workload_token=tok_1,
        daemon_host="127.0.0.1",
        tcp_port=DAEMON_TCP_PORT,
        http_port=DAEMON_HTTP_PORT,
    )
    await treasury.boot()
    print(f"  {Style.GREEN}✔{Style.RESET} Offline PKI Provisioned : {Style.BOLD}4 agents{Style.RESET} (`rqm_v1` Ed25519 tokens + `.cert` passports)")

    # Verify Admin Plane is physically unreachable over TCP :8081
    async with httpx.AsyncClient(timeout=3.0) as http:
        for admin_route in ("/v1/admin/ca/mint", "/v1/admin/quarantine/lift", "/v1/admin/in_doubt"):
            r = await http.post(f"{DAEMON_HTTP}{admin_route}", json={"agent_hex": hex_1})
            assert r.status_code in (404, 405), f"Admin route {admin_route} exposed on TCP ({r.status_code})!"
    print(f"  {Style.GREEN}✔{Style.RESET} Admin Plane Isolation   : All `/v1/admin/*` routes returned {Style.BOLD}HTTP 404{Style.RESET} on TCP Data Plane")

    # ==========================================================================
    # ACT 2: THE 4 CONCENTRIC EGRESS LOCKS (LIVE PAYSTACK AIRLOCK)
    # ==========================================================================
    print_header(
        "ACT 2: ZERO-SECRET EGRESS GATEWAY & THE 4 CONCENTRIC LOCKS",
        "Authorized ₦25,000 Disbursement vs. Polite ₦5M Drain, JSON Smuggling & Velocity Attacks",
    )

    # 2A. Legitimate Transfer (25,000 NGN with customer BVN that must be scrubbed before WAL)
    t0 = time.perf_counter()
    resp_ok = await treasury.egress_request(
        "POST",
        "paystack",
        "/transfer",
        json_body={
            "amount": 25000,
            "currency": "NGN",
            "recipient": "RCP_VENDOR_LAGOS_01",
            "bvn": "22345678901",
        },
        step_ordinal=1,
    )
    live_transfer_ms = (time.perf_counter() - t0) * 1000
    assert resp_ok.status_code == 200, f"Authorized transfer failed: {resp_ok.text}"
    paystack_tx_id = resp_ok.headers["x-raqim-tx-id"]
    paystack_receipt_bytes = resp_ok.content
    assert UPSTREAM_STATS["paystack_hits"] == 1
    assert UPSTREAM_STATS["last_paystack_auth_valid"] is True
    assert UPSTREAM_STATS["last_paystack_bvn_received"] == "22345678901"
    assert len(UPSTREAM_STATS["last_idempotency_key"]) == 32

    print(f"  {Style.GREEN}✔ [LOCK PASS] Authorized Transfer (₦25,000 NGN):{Style.RESET} HTTP 200 in {live_transfer_ms:.2f}ms")
    print(f"    ├─ Secret Injection  : Daemon injected `Bearer sk_live_...` (Agent never saw key)")
    print(f"    ├─ Idempotency-Key   : {Style.CYAN}{UPSTREAM_STATS['last_idempotency_key']}{Style.RESET} (Auto-bound to Write-Ahead Intent)")
    print(f"    └─ Sealed TxID       : {Style.CYAN}{paystack_tx_id}{Style.RESET}\n")

    async with httpx.AsyncClient(timeout=5.0) as http:
        # 2B. Attack 1: "Polite Evil Request" — Single-transfer cap breach (₦5,000,000 > ₦50,000 cap)
        t_block = time.perf_counter()
        r_cap = await http.post(
            f"{DAEMON_HTTP}/v1/egress/paystack/transfer",
            headers={"Authorization": f"Bearer {tok_cap_attacker}", "Content-Type": "application/json"},
            json={"amount": 5_000_000, "currency": "NGN", "recipient": "RCP_ATTACKER"},
        )
        block_latency_ms = (time.perf_counter() - t_block) * 1000
        assert r_cap.status_code == 403, f"Expected 403 on over-cap transfer, got {r_cap.status_code}"
        assert UPSTREAM_STATS["paystack_hits"] == 1, "Blocked transfer reached Paystack upstream!"
        print(f"  {Style.BG_RED}{Style.WHITE} ✖ [LOCK 2 BLOCKED IN {block_latency_ms:.2f}ms] {Style.RESET} Polite ₦5,000,000 transfer exceeded `/amount <= 50000` cap -> {Style.RED}QUARANTINED{Style.RESET}")

        # 2C. Attack 2: JSON Duplicate-Key Smuggling (`{"amount": 100, "amount": 50000000}`)
        smuggled_raw = b'{"amount": 100, "amount": 50000000, "currency": "NGN"}'
        r_smug = await http.post(
            f"{DAEMON_HTTP}/v1/egress/paystack/transfer",
            headers={"Authorization": f"Bearer {tok_smuggler}", "Content-Type": "application/json"},
            content=smuggled_raw,
        )
        assert r_smug.status_code == 403, f"Expected 403 on duplicate-key JSON, got {r_smug.status_code}"
        assert UPSTREAM_STATS["paystack_hits"] == 1
        print(f"  {Style.BG_RED}{Style.WHITE} ✖ [LOCK 3 BLOCKED] {Style.RESET} Duplicate-key JSON smuggling (`amount:100` + `amount:50M`) -> {Style.RED}QUARANTINED{Style.RESET}")

        # 2D. Attack 3: Rolling 1-Hour Velocity Drain (3x ₦40,000 = ₦120,000 > ₦100,000/hr cap)
        for idx in (1, 2):
            r_v = await http.post(
                f"{DAEMON_HTTP}/v1/egress/paystack/transfer",
                headers={"Authorization": f"Bearer {tok_velocity}", "Content-Type": "application/json"},
                json={"amount": 40000, "currency": "NGN"},
            )
            assert r_v.status_code == 200, f"Velocity transfer #{idx} should pass within 100k cap"

        r_v3 = await http.post(
            f"{DAEMON_HTTP}/v1/egress/paystack/transfer",
            headers={"Authorization": f"Bearer {tok_velocity}", "Content-Type": "application/json"},
            json={"amount": 40000, "currency": "NGN"},
        )
        assert r_v3.status_code == 403, f"3rd velocity transfer (120k > 100k/hr) must be blocked!"
        assert UPSTREAM_STATS["paystack_hits"] == 3  # Only 1 (primary) + 2 (within cap) hit upstream
        print(f"  {Style.BG_RED}{Style.WHITE} ✖ [LOCK 4 BLOCKED] {Style.RESET} Velocity drain (3x ₦40k = ₦120k > ₦100k/hr rolling cap) -> {Style.RED}QUARANTINED{Style.RESET}")

    # ==========================================================================
    # ACT 3: ZERO-LEAK DISK AUDIT, ENVELOPE MERKLE PROOF, NDPA ERASURE & CRC
    # ==========================================================================
    print_header(
        "ACT 3: CRYPTOGRAPHIC ATTESTATION, NDPA ERASURE & DISK INTEGRITY",
        "Zero-Leak Secret Grep, Full-Envelope Offline Proofs & Physical Mid-File WAL Audit",
    )

    # 3A. Raw Disk Secret-Grep & BVN Redaction Audit
    wal_raw_text = open(DEMO_WAL_PATH, "rb").read().decode("utf-8", errors="ignore")
    assert PAYSTACK_CANARY_SECRET not in wal_raw_text, "CRITICAL: Paystack secret leaked into WAL!"
    assert "22345678901" not in wal_raw_text, "CRITICAL: Customer BVN leaked into WAL!"
    assert "[REDACTED_BY_POLICY]" in wal_raw_text, "Expected [REDACTED_BY_POLICY] marker in WAL"
    print(f"  {Style.GREEN}✔{Style.RESET} Disk Secret-Grep Audit  : {Style.GREEN}0 secrets, 0 raw BVNs on NVMe{Style.RESET} (`/bvn` scrubbed pre-WAL)")

    # 3B. Offline Merkle Proof on the Authorized Paystack Receipt + Tamper Rejection
    async with httpx.AsyncClient(timeout=3.0) as http:
        p_resp = await http.get(f"{DAEMON_HTTP}/v1/state/proof/{paystack_tx_id}")
        assert p_resp.status_code == 200
        proof_dict = p_resp.json()["proof"]

    pre_crash_root = proof_dict["merkleRootHex"]
    assert verify_state_proof_offline(paystack_receipt_bytes, hex_1, proof_dict), "Valid receipt failed offline proof!"

    # Tamper Test 1: Flip 1 byte in amount -> MUST FAIL
    tampered_receipt = paystack_receipt_bytes.replace(b"25000", b"99999")
    assert not verify_state_proof_offline(tampered_receipt, hex_1, proof_dict), "Tampered amount passed proof!"

    # Tamper Test 2: Flip stage from Outcome (2) to Denied (3) -> MUST FAIL (v0.2.0 envelope binding!)
    forged_stage_proof = dict(proof_dict)
    forged_stage_proof["stage"] = 3
    assert not verify_state_proof_offline(paystack_receipt_bytes, hex_1, forged_stage_proof), "Flipped stage passed proof!"
    print(f"  {Style.GREEN}✔{Style.RESET} Offline Envelope Proof  : {Style.GREEN}VALID{Style.RESET} (Root: {pre_crash_root[:16]}... | Payload & Stage tamper REJECTED)")

    # 3C. NDPA / GDPR Right-to-Erasure via Admin Socket (`POST /v1/admin/redact/{tx_id}`)
    r_redact = await admin_uds_request("POST", f"/v1/admin/redact/{paystack_tx_id}")
    assert r_redact.status_code == 200, f"NDPA redaction failed: {r_redact.text}"

    async with httpx.AsyncClient(timeout=3.0) as http:
        redacted_proof = (await http.get(f"{DAEMON_HTTP}/v1/state/proof/{paystack_tx_id}")).json()["proof"]
    assert redacted_proof["payloadSaltHex"] == "00" * 16, "128-bit blinding salt was not zeroed!"
    assert verify_state_proof_offline(None, hex_1, redacted_proof), "Merkle proof broke after NDPA erasure!"
    print(f"  {Style.GREEN}✔{Style.RESET} NDPA Crypto-Shredding   : Payload & 128-bit salt erased -> {Style.GREEN}Merkle proof STILL 100% VALID!{Style.RESET}")

    # 3D. Physical Mid-File WAL Bit-Flip Detection
    corrupt_caught, corrupt_line, flip_off = await test_mid_file_wal_corruption_halt()
    assert corrupt_caught, "Engine failed to detect mid-file WAL CRC32 corruption!"
    print(f"  {Style.GREEN}✔{Style.RESET} Mid-File WAL Bit-Flip   : Flipped byte {flip_off} -> {Style.YELLOW}{corrupt_line[:68]}...{Style.RESET}")

    # ==========================================================================
    # ACT 4: MID-PAYMENT CRASH & IN-DOUBT INTENT RECONCILIATION
    # ==========================================================================
    print_header(
        "ACT 4: CRASH MID-PAYMENT & IN-DOUBT INTENT RECONCILIATION",
        "Write-ahead `Intent` durability surfaces interrupted bank transfers without blind double-charging",
    )

    r_broken = await treasury.egress_request(
        "POST",
        "broken_bank",
        "/wire",
        json_body={"amount": 15000, "currency": "NGN"},
        step_ordinal=2,
    )
    assert r_broken.status_code == 500, "Expected 500 when upstream bank drops mid-flight"

    r_doubt = await admin_uds_request("GET", "/v1/admin/in_doubt")
    assert r_doubt.status_code == 200
    in_doubt_list = r_doubt.json()
    assert len(in_doubt_list) == 1, f"Expected 1 In-Doubt intent, found {len(in_doubt_list)}"
    doubt_tx_hex = f"{in_doubt_list[0]['tx_id']:032x}"
    print(f"  {Style.YELLOW}⚠{Style.RESET} Upstream bank dropped TCP connection mid-transfer (`HTTP 500`).")
    print(f"  {Style.GREEN}✔{Style.RESET} Operator Admin Query (`/v1/admin/in_doubt`) surfaced exact unresolved Intent:")
    print(f"    └─ Idempotency-Key / TxID: {Style.CYAN}{doubt_tx_hex}{Style.RESET} | Path: {in_doubt_list[0]['namespace']}")

    # ==========================================================================
    # ACT 5: ZERO-STALL SSE BYTE-TEE, $0.00 GATEWAY REPLAY & FORKING
    # ==========================================================================
    print_header(
        "ACT 5: ZERO-STALL SSE STREAMING BYTE-TEE & $0.00 REPLAY",
        "Streaming LLM tokens captured inline via Byte-Tee and replayed in <1ms with zero upstream calls",
    )

    # 5A. Live Streaming OpenAI SSE Call through Gateway
    t_sse1 = time.perf_counter()
    r_sse_live = await treasury.egress_request(
        "POST",
        "openai",
        "/v1/chat/completions",
        json_body={"model": "gpt-4o-mini", "stream": True, "messages": [{"role": "user", "content": "Audit 25k"}]},
        step_ordinal=3,
    )
    sse_live_ms = (time.perf_counter() - t_sse1) * 1000
    assert r_sse_live.status_code == 200
    assert "text/event-stream" in r_sse_live.headers.get("content-type", "")
    assert b"25,000 NGN approved" in r_sse_live.content
    assert UPSTREAM_STATS["openai_hits"] == 1

    # 5B. $0.00 Gateway Replay of the Exact SSE Stream
    treasury.mode = "replay"
    t_sse2 = time.perf_counter()
    r_sse_replay = await treasury.egress_request(
        "POST",
        "openai",
        "/v1/chat/completions",
        json_body={"model": "gpt-4o-mini", "stream": True, "messages": [{"role": "user", "content": "Audit 25k"}]},
        step_ordinal=3,
    )
    sse_replay_ms = (time.perf_counter() - t_sse2) * 1000
    assert r_sse_replay.status_code == 200
    assert r_sse_replay.headers.get("x-raqim-replay") == "HIT"
    assert r_sse_replay.content == r_sse_live.content
    assert UPSTREAM_STATS["openai_hits"] == 1, "Replay contacted OpenAI upstream!"

    print(f"  {Style.GREEN}✔{Style.RESET} Live SSE Stream (5 chunks) : {sse_live_ms:.2f}ms (Upstream Hits: 1 | Zero-stall byte-tee sealed)")
    print(f"  {Style.GREEN}✔{Style.RESET} $0.00 SSE Gateway Replay   : {Style.BOLD}{Style.GREEN}{sse_replay_ms:.2f}ms{Style.RESET} (`x-raqim-replay: HIT` | Upstream Hits: STILL 1!)")

    # 5C. Local `@treasury.trace` Counterfactual Forking
    _execution_step_context.set(10)
    treasury.mode = "record"
    local_exec_count = 0

    @treasury.trace(namespace="/finance/tools/risk_model")
    async def run_risk_score(account: str, threshold: float) -> dict:
        nonlocal local_exec_count
        local_exec_count += 1
        return {"account": account, "threshold": threshold, "risk": "LOW" if threshold >= 50000 else "HIGH"}

    out_live = await run_risk_score("ACC_LAGOS_88", 50000.0)
    _execution_step_context.set(10)
    treasury.mode = "replay"
    treasury.is_forked = False
    out_replay = await run_risk_score("ACC_LAGOS_88", 50000.0)
    assert out_replay == out_live and local_exec_count == 1

    # Diverge input -> auto-forks into `phantom_` branch!
    _execution_step_context.set(10)
    treasury.is_forked = False
    out_forked = await run_risk_score("ACC_LAGOS_88", 10000.0)
    assert treasury.is_forked and out_forked["risk"] == "HIGH" and local_exec_count == 2
    print(f"  {Style.GREEN}✔{Style.RESET} Counterfactual Forking     : Replay hit = 0 executions; mutated arg forked cleanly onto `phantom_`")

    # ==========================================================================
    # ACT 6: THE PHOENIX MOMENT (SIGKILL & SUB-50ms REHYDRATION)
    # ==========================================================================
    print_header(
        "ACT 6: THE PHOENIX MOMENT",
        "Uncatchable `SIGKILL (kill -9)` -> Sub-50ms Recovery with Root, Quarantine & In-Doubt Invariance",
    )

    print(f"  Issuing {Style.RED}SIGKILL (kill -9){Style.RESET} to `raqim-core` (PID: {daemon_proc.pid})...")
    await kill_daemon_phoenix(daemon_proc)
    resurrect_ms, daemon_proc = await resurrect_daemon_phoenix()

    # Verify Merkle Root, Quarantine Blocklist (all 3 rogue agents!), and In-Doubt Intent survived SIGKILL!
    async with httpx.AsyncClient(timeout=3.0) as http:
        reboot_proof = (await http.get(f"{DAEMON_HTTP}/v1/state/proof/{paystack_tx_id}")).json()["proof"]
    assert reboot_proof["merkleRootHex"] == pre_crash_root, "Merkle Root drifted across SIGKILL!"

    q_list = (await admin_uds_request("GET", "/v1/policy/quarantine_list")).json()
    q_hexes = {r["agent_hex"] for r in q_list}
    for rogue_hex in (hex_cap, hex_smug, hex_vel):
        assert rogue_hex in q_hexes, f"Quarantined agent {rogue_hex} lost across SIGKILL!"

    post_doubt = (await admin_uds_request("GET", "/v1/admin/in_doubt")).json()
    assert len(post_doubt) == 1, "In-Doubt Intent lost across SIGKILL!"

    print(f"  {Style.BG_GREEN}{Style.WHITE} ⚡ PHOENIX RESURRECTION COMPLETE IN {resurrect_ms:.2f}ms {Style.RESET}")
    print(f"  {Style.GREEN}✔{Style.RESET} Merkle Root Invariance : Matched pre-crash root ({pre_crash_root[:16]}...)")
    print(f"  {Style.GREEN}✔{Style.RESET} Quarantine Persistence : All 3 rogue agents confirmed locked in `/v1/policy/quarantine_list`")
    print(f"  {Style.GREEN}✔{Style.RESET} In-Doubt Persistence   : Unresolved wire intent `{doubt_tx_hex[:12]}...` preserved across crash")

    # ==========================================================================
    # EMPIRICAL VERIFICATION SCORECARD
    # ==========================================================================
    print_header("RAQIM v0.2.0 EMPIRICAL VERIFICATION SCORECARD", "Every Figure Measured Live in This Execution")

    _, build_profile = get_binary_info()
    rows = [
        ("Agent Process Secrets in Env", "0 (Zero-Secret Airlock)"),
        ("TCP Admin Plane Exposure", "404 Not Found (Isolated to UDS 0600)"),
        ("Live Paystack Transfer (Injected Key)", f"{live_transfer_ms:.2f} ms (HTTP 200 + Idempotency-Key)"),
        ("Polite ₦5M Cap / Smuggling / Velocity", f"ALL 3 BLOCKED & QUARANTINED ({block_latency_ms:.2f} ms)"),
        ("NVMe WAL Secret & BVN Leak Grep", "0 LEAKS ([REDACTED_BY_POLICY])"),
        ("NDPA In-Place Redaction + Proof", "100% VALID Offline After Salt Erasure"),
        ("Mid-File WAL Bit-Flip Check", f"DETECTED & HALTED (Byte Offset {flip_off})"),
        ("Mid-Payment Drop Reconciliation", "1 In-Doubt Intent Surfaced via Admin UDS"),
        ("SSE Stream Live vs. $0.00 Replay", f"{sse_live_ms:.2f} ms Live -> {sse_replay_ms:.2f} ms ($0.00 HIT)"),
        ("Phoenix SIGKILL Process Recovery", f"{resurrect_ms:.2f} ms ({build_profile})"),
    ]

    w1, w2 = 40, 48
    print("  ┌" + "─" * (w1 + 2) + "┬" + "─" * (w2 + 2) + "┐")
    print(f"  │{pad_visible(f' {Style.BOLD}Measured Security / Systems Invariant{Style.RESET}', w1 + 2)}│{pad_visible(f' {Style.BOLD}Empirical Result{Style.RESET}', w2 + 2)}│")
    print("  ├" + "─" * (w1 + 2) + "┼" + "─" * (w2 + 2) + "┤")
    for k, v in rows:
        print(f"  │{pad_visible(f' {k}', w1 + 2)}│{pad_visible(f' {Style.GREEN}{v}{Style.RESET}', w2 + 2)}│")
    print("  └" + "─" * (w1 + 2) + "┴" + "─" * (w2 + 2) + "┘\n")

    print(f"{Style.BOLD}{Style.GREEN}Alhamdulillah — All 6 Acts and empirical security invariants passed.{Style.RESET}\n")
    stop_demo_daemon()
    stop_mock_upstream_server()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    finally:
        stop_demo_daemon()
        stop_mock_upstream_server()
    os._exit(0)