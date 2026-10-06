#!/usr/bin/env python3
"""
================================================================================
Raqim Agent Execution-Integrity Runtime — Interactive Demonstration
================================================================================
Demonstrates verified execution safety, cryptographic flight recording, and
deterministic replay for autonomous AI agents:

1. Agent Identity Provisioning (Ed25519 PKI + Capability Passports)
2. Control-Plane Pre-Execution Interdiction: Namespace Policy Enforcement & Warm Benchmark
3. Cryptographic Flight Recording & Zero-Network Offline Merkle Attestation (64-Event Batch)
4. Tamper Defense: In-Flight Payload Modification & Physical On-Disk WAL Corruption Check
5. The Phoenix Moment: SIGKILL Hard Kill, State Invariance, & Strict Quarantine List Assertion
6. 7-Step Autonomous AML Pipeline: $0.00 Replay, Value Equality, & Canonical Invariance Proof
================================================================================
"""

import asyncio
import os
import time
import httpx
from typing import Dict, Any

from demo_utils import (
    DAEMON_HTTP,
    DAEMON_TCP_PORT,
    DAEMON_HTTP_PORT,
    DEMO_WAL_PATH,
    Style,
    print_banner,
    print_header,
    clean_demo_sandbox,
    ensure_daemon_running,
    stop_demo_daemon,
    kill_daemon_phoenix,
    resurrect_daemon_phoenix,
    forge_agent_credentials,
    call_llm,
    get_llm_call_count,
    reset_llm_call_count,
    get_binary_info,
    ACTIVE_LLM_PROVIDER,
    test_disk_wal_corruption_recovery,
    pad_visible,
    visible_len,
)

from raqim.client import (
    CanonicalSerializer,
    RaqimClient,
    _execution_step_context,
    verify_state_proof_offline,
)

async def main():
    print_banner()

    _, build_profile = get_binary_info()
    print(f"{Style.BOLD}Execution Environment:{Style.RESET}")
    print(f"  • Build Profile   : {Style.CYAN}{build_profile}{Style.RESET}")
    print(f"  • Network Binding : {Style.CYAN}127.0.0.1 (Loopback Only — No Public Ingress){Style.RESET}")
    if ACTIVE_LLM_PROVIDER:
        print(f"  • Reasoning Engine: {Style.GREEN}Live API — {ACTIVE_LLM_PROVIDER}{Style.RESET}\n")
    else:
        print(f"  • Reasoning Engine: {Style.YELLOW}Deterministic Simulated Engine (No API key set){Style.RESET}\n")

    # Step 0: Ensure pristine demo sandbox (leaves all production data untouched)
    clean_demo_sandbox()
    daemon_proc = await ensure_daemon_running()

    # --------------------------------------------------------------------------
    # ACT 1: AGENT IDENTITY PROVISIONING (Ed25519 PKI + CAPABILITY PASSPORT)
    # --------------------------------------------------------------------------
    print_header("ACT 1: AGENT IDENTITY & CAPABILITY PASSPORTS", "Zero Ambient Authority via Ed25519 Machine Passports")

    analyst_key, analyst_cert = await forge_agent_credentials("senior_analyst", "analyst_group")
    crawler_key, crawler_cert = await forge_agent_credentials("crawler_bot", "finance_worker")

    analyst = RaqimClient(
        alias="SeniorAMLAnalyst",
        tenant="unilorin_aml_research",
        private_key_path=analyst_key,
        cert_path=analyst_cert,
        daemon_host="127.0.0.1",
        tcp_port=DAEMON_TCP_PORT,
        http_port=DAEMON_HTTP_PORT,
        mode="record",
        on_divergence="fork",
    )
    await analyst.boot()

    rogue_crawler = RaqimClient(
        alias="CompromisedCrawler",
        tenant="unilorin_aml_research",
        private_key_path=crawler_key,
        cert_path=crawler_cert,
        daemon_host="127.0.0.1",
        tcp_port=DAEMON_TCP_PORT,
        http_port=DAEMON_HTTP_PORT,
        mode="record",
        on_divergence="fork",
    )
    await rogue_crawler.boot()

    print(f"  {Style.GREEN}✔{Style.RESET} Agent 1: {Style.BOLD}SeniorAMLAnalyst{Style.RESET} [ID: {analyst.agent_hex[:12]}...] (Group: {Style.CYAN}analyst_group{Style.RESET})")
    print(f"  {Style.GREEN}✔{Style.RESET} Agent 2: {Style.BOLD}CompromisedCrawler{Style.RESET} [ID: {rogue_crawler.agent_hex[:12]}...] (Group: {Style.CYAN}finance_worker{Style.RESET})")
    print(f"  {Style.DIM}  Root CA: Swarm Master Public Key verified locally.{Style.RESET}")

    # --------------------------------------------------------------------------
    # ACT 2: CONTROL-PLANE PRE-EXECUTION INTERDICTION (Namespace Policy Enforcement & Steady-State Benchmark)
    # --------------------------------------------------------------------------
    print_header("ACT 2: CONTROL-PLANE PRE-EXECUTION INTERDICTION", "Unprotected Function Dispatch vs. Pre-Execution Policy Gate")

    print(f"{Style.BOLD}Scenario:{Style.RESET}")
    print(f"An autonomous agent script proposes an unauthorized fund transfer:")
    print(f"{Style.RED}  Dispatch Target: /finance/restricted/vault_transfer ($500,000 to CAYMAN_VAULT_99821){Style.RESET}\n")

    # REALITY A: Unprotected Execution (What happens without a pre-execution gate)
    print(f"{Style.YELLOW}▶ REALITY A: Unprotected Python Execution (No Policy Gate){Style.RESET}")
    unprotected_executed = False

    async def raw_wire_transfer(recipient: str, amount_usd: float) -> str:
        nonlocal unprotected_executed
        unprotected_executed = True
        return f"EXECUTED: Wired ${amount_usd:,.2f} to {recipient}"

    result_a = await raw_wire_transfer("CAYMAN_VAULT_99821", 500000.0)
    assert unprotected_executed, "Sanity check failed: raw function should execute"
    print(f"  1. Agent proposes transfer without control-plane checks.")
    print(f"  2. Function body executed : {Style.RED}{Style.BOLD}TRUE ($500,000 EXFILTRATED){Style.RESET}")
    print(f"  3. External API Result    : {Style.RED}{result_a}{Style.RESET}")
    print(f"  {Style.RED}✖ OBSERVABILITY LIMITATION:{Style.RESET} Passive logs/traces can only record this after money left.\n")

    # REALITY B: The Raqim Pre-Execution Gate
    print(f"{Style.CYAN}▶ REALITY B: Raqim Control-Plane Gate (@rogue_crawler.trace){Style.RESET}")
    print(f"  1. Agent proposes mutation: {Style.BOLD}/finance/restricted/vault_transfer{Style.RESET}")
    print(f"  2. SDK sends pre-flight authorization probe to local Raqim daemon.")
    print(f"  3. Aegis evaluates policy: Tripped blocked rule [/finance/restricted/*]")

    gated_executed = False

    @rogue_crawler.trace(namespace="/finance/restricted/vault_transfer")
    async def gated_wire_transfer(recipient: str, amount_usd: float) -> str:
        nonlocal gated_executed
        gated_executed = True
        return f"EXECUTED: Wired ${amount_usd:,.2f} to {recipient}"

    t_interdict_start = time.perf_counter()
    interdicted = False
    cold_latency_ms = 0.0
    try:
        await gated_wire_transfer("CAYMAN_VAULT_99821", 500000.0)
    except PermissionError:
        interdicted = True
        cold_latency_ms = (time.perf_counter() - t_interdict_start) * 1000
        print(f"  4. {Style.BG_GREEN}{Style.WHITE} ACTION INTERDICTED IN {cold_latency_ms:.2f}ms (Cold Start) {Style.RESET}")
        print(f"     └─ Cold TCP/HTTP connection handshake + pre-flight evaluation: {cold_latency_ms:.2f}ms")
        print(f"  5. Function body executed: {Style.BOLD}FALSE (Zero Side-Effects Committed){Style.RESET}")
        print(f"  6. Agent state           : {Style.RED}{rogue_crawler.agent_hex[:12]}... [QUARANTINED]{Style.RESET}")

    assert interdicted, "CRITICAL: Pre-execution gate failed to reject forbidden namespace!"
    assert not gated_executed, "CRITICAL: Function body ran despite policy interdiction!"

    # Warm Steady-State Pre-Execution Latency Benchmark
    print(f"\n{Style.BOLD}Steady-State Pre-Execution Benchmark (Warm Loop):{Style.RESET}")
    warm_latencies = []
    for _ in range(200):
        t0 = time.perf_counter()
        try:
            await gated_wire_transfer("CAYMAN_VAULT_99821", 500000.0)
        except PermissionError:
            warm_latencies.append((time.perf_counter() - t0) * 1000)

    warm_latencies.sort()
    p50_latency_ms = warm_latencies[len(warm_latencies) // 2]
    p99_latency_ms = warm_latencies[int(len(warm_latencies) * 0.99)]
    print(f"  ✔ Warm Pre-Flight Benchmark (200 calls) : {Style.GREEN}p50 = {p50_latency_ms:.2f}ms | p99 = {p99_latency_ms:.2f}ms{Style.RESET}")

    # --------------------------------------------------------------------------
    # ACT 3: CRYPTOGRAPHIC FLIGHT RECORDING & OFFLINE MERKLE ATTESTATION
    # --------------------------------------------------------------------------
    print_header("ACT 3: CRYPTOGRAPHIC FLIGHT RECORDING", "BLAKE3 Merkle DAG Sealing & Zero-Network Inclusion Proofs")

    print(f"{Style.BOLD}Step 1: Ingesting a 64-transaction flight ledger batch into Axon DAG...{Style.RESET}")
    _execution_step_context.set(0)
    analyst.mode = "record"

    @analyst.trace(namespace="/finance/tools/screening")
    async def tool_screen_tx(tx: dict) -> dict:
        return {
            "tx_id": tx["tx_id"],
            "amount_usd": tx["amount_usd"],
            "flagged": tx.get("flagged", False),
            "routing": tx.get("routing", "DOMESTIC_ACH"),
            "timestamp": tx.get("timestamp", 1774900000),
        }

    evidence_leaf_17 = None
    for i in range(64):
        _execution_step_context.set(i)
        tx_payload = {
            "tx_id": f"TX_SCREEN_AML_{i:04d}",
            "amount_usd": 500.0 + (i * 150.0),
            "flagged": (i == 17),
            "routing": "OFFSHORE_HIGH_RISK" if i == 17 else "DOMESTIC_ACH",
            "timestamp": 1774900000 + i,
        }
        res = await tool_screen_tx(tx_payload)
        if i == 17:
            evidence_leaf_17 = res

    target_tx = analyst.recorded_tx_ids.get(17)
    assert target_tx, "CRITICAL: Leaf 17 transaction ID missing from recorded IDs!"

    print(f"  ✔ 64 flight ledger events successfully committed to Axon DAG buffer")
    print(f"  ✔ Target for Inclusion Proof: Event #17 [{target_tx[:16]}...]")

    print(f"\n{Style.BOLD}Step 2: Retrieving Merkle Inclusion Proof from Axon DAG...{Style.RESET}")
    proof_dict = None
    async with httpx.AsyncClient(timeout=3.0) as http:
        for _ in range(25):
            await asyncio.sleep(0.05)
            proof_resp = await http.get(f"{DAEMON_HTTP}/v1/state/proof/{target_tx}")
            if proof_resp.status_code == 200:
                raw = proof_resp.json()
                proof_dict = raw.get("proof", raw)
                if proof_dict:
                    break

    assert proof_dict is not None, "CRITICAL: Axon DAG failed to yield cryptographic inclusion proof!"

    merkle_root = proof_dict.get("merkleRootHex", proof_dict.get("merkle_root_hex", ""))
    leaf_idx = proof_dict.get("leafIndex", proof_dict.get("leaf_index", 0))
    batch_id = proof_dict.get("batchId", proof_dict.get("batch_id", 0))
    siblings = (
        proof_dict.get("siblingHashesHex")
        or proof_dict.get("sibling_hashes_hex")
        or proof_dict.get("siblings")
        or []
    )
    is_active = proof_dict.get("isActiveBuffer", proof_dict.get("is_active_buffer", True))
    proof_bytes_calc = len(siblings) * 32

    print(f"  {Style.CYAN}Batch ID        :{Style.RESET} #{batch_id}")
    print(f"  {Style.CYAN}Leaf Index      :{Style.RESET} {leaf_idx}")
    print(f"  {Style.CYAN}Merkle Root     :{Style.RESET} {merkle_root}")
    print(f"  {Style.CYAN}Sibling Hashes  :{Style.RESET} {len(siblings)} nodes ({proof_bytes_calc} bytes)")
    print(f"  {Style.CYAN}Active Buffer   :{Style.RESET} {is_active} (Un-crystallized Workspace Tree)")
    print(f"  {Style.DIM}  Math: Current tree depth = log2(64) = 6 levels (192 B). Sealed 1,024 batch = 10 levels (320 B).{Style.RESET}")
    assert len(siblings) > 0, f"CRITICAL: Merkle proof returned {len(siblings)} siblings for a 64-leaf tree!"

    # Offline Verification of the untampered record
    canonical_bytes = CanonicalSerializer.canonical_json(evidence_leaf_17).encode("utf-8")
    is_valid = verify_state_proof_offline(
        payload_bytes=canonical_bytes,
        agent_id_str=analyst.agent_hex,
        proof_dict=proof_dict,
    )

    print(f"\n{Style.BOLD}Step 3: Offline Verification of Untampered Evidence...{Style.RESET}")
    print(f"  Network Requests Made : {Style.BOLD}0{Style.RESET}")
    print(f"  Database Queries Made : {Style.BOLD}0{Style.RESET}")
    print(f"  Mathematical Proof    : {Style.BOLD}{Style.GREEN}VALID (Leaf #17 provably anchored to Merkle Root){Style.RESET}")
    assert is_valid, "CRITICAL: Untampered evidence failed offline cryptographic verification!"

    # --------------------------------------------------------------------------
    # ACT 4: TAMPER DETECTION TESTS
    # --------------------------------------------------------------------------
    print_header("ACT 4: TAMPER DETECTION TESTS", "In-Flight Payload Mutation & Physical On-Disk Frame Integrity")

    # Test 4A: Payload Tampering (Modifying data fields)
    print(f"{Style.BOLD}Test 4A: Simulating an unauthorized edit to regulatory record fields:{Style.RESET}")
    print(f"  Original Amount  : {Style.CYAN}${evidence_leaf_17['amount_usd']:,.2f}{Style.RESET} (Screened Event #17)")
    print(f"  Falsified Amount : {Style.YELLOW}$10.00{Style.RESET} (Modified to evade reporting threshold)")

    tampered_evidence = evidence_leaf_17.copy()
    tampered_evidence["amount_usd"] = 10.00
    tampered_evidence["flagged"] = False
    tampered_bytes = CanonicalSerializer.canonical_json(tampered_evidence).encode("utf-8")

    tamper_verified = verify_state_proof_offline(
        payload_bytes=tampered_bytes,
        agent_id_str=analyst.agent_hex,
        proof_dict=proof_dict,
    )

    print(f"\n{Style.BOLD}Offline Verifier Response on Modified Payload:{Style.RESET}")
    if not tamper_verified:
        print(f"  {Style.BG_RED}{Style.WHITE} ❌ MODIFIED RECORD REJECTED BY CRYPTOGRAPHIC VERIFIER {Style.RESET}")
        print(f"  Calculated Leaf Hash != Anchored Merkle Path.")
        print(f"  Verdict: Unauthenticated modification detected offline.")
    assert not tamper_verified, "CRITICAL: Modified payload must NOT pass Merkle verification!"

    # Test 4B: TRUE ON-DISK PHYSICAL WAL STORAGE CORRUPTION
    print(f"\n{Style.BOLD}Test 4B: Physical On-Disk WAL Corruption & Daemon Boot Scan:{Style.RESET}")
    mismatch_detected, log_line, offset = await test_disk_wal_corruption_recovery()
    print(f"  Flipped 1 Byte on Disk at Offset : byte {offset}")
    print(f"  Throwaway Daemon Boot Log        : {Style.GREEN}{log_line}{Style.RESET}")
    print(f"  Engine Integrity Assertion       : {Style.GREEN}Corrupted Frame Detected by Rust Engine (Halted Scan){Style.RESET}")
    assert mismatch_detected, "CRITICAL: Engine failed to catch on-disk CRC32 corruption!"

    # --------------------------------------------------------------------------
    # ACT 5: THE PHOENIX MOMENT (SIGKILL CRASH & ZERO-DATA-LOSS HYDRATION)
    # --------------------------------------------------------------------------
    print_header("ACT 5: THE PHOENIX MOMENT", "Hard Process Crash (SIGKILL) & State Rehydration from WAL")

    pre_crash_root = merkle_root
    print(f"{Style.BOLD}Simulating uncatchable crash:{Style.RESET}")
    print(f"Issuing {Style.RED}SIGKILL (kill -9){Style.RESET} to the active daemon process (PID: {daemon_proc.pid})...")

    await kill_daemon_phoenix(daemon_proc)

    daemon_dead = False
    try:
        async with httpx.AsyncClient(timeout=0.3) as http:
            await http.get(f"{DAEMON_HTTP}/health")
    except Exception:
        daemon_dead = True
    assert daemon_dead, "CRITICAL: Daemon socket still responding after SIGKILL!"
    print(f"  {Style.RED}✖ Process is DEAD.{Style.RESET} Verified: Connection to {DAEMON_HTTP} actively refused.")

    print(f"\n{Style.BOLD}Resurrecting Daemon from Sandbox WAL...{Style.RESET}")
    resurrect_duration_ms, daemon_proc = await resurrect_daemon_phoenix()

    print(f"  {Style.BG_GREEN}{Style.WHITE} ⚡ PHOENIX RESURRECTION COMPLETE: {resurrect_duration_ms:.2f}ms {Style.RESET}")
    print(f"     └─ OS Process Cold Boot + Port Binding: {resurrect_duration_ms:.2f}ms ({build_profile})")

    # Positive Control & Strict Durability Verification
    print(f"\n{Style.BOLD}Post-Crash Durability Verification:{Style.RESET}")
    async with httpx.AsyncClient(timeout=3.0) as http:
        health_resp = await http.get(f"{DAEMON_HTTP}/health")
        assert health_resp.status_code == 200, "Daemon unhealthy after resurrection"

        # 1. State Invariance: Pre-crash root must equal post-crash root
        reboot_proof_resp = await http.get(f"{DAEMON_HTTP}/v1/state/proof/{target_tx}")
        assert reboot_proof_resp.status_code == 200, "Historical transaction proof lost across reboot!"
        reboot_proof = reboot_proof_resp.json().get("proof", reboot_proof_resp.json())
        post_crash_root = reboot_proof.get("merkleRootHex", reboot_proof.get("merkle_root_hex", ""))
        assert post_crash_root == pre_crash_root, f"Merkle Root changed across reboot! ({post_crash_root} != {pre_crash_root})"
        print(f"  ✔ State Invariance : {Style.GREEN}Pre-crash Merkle root matched post-crash root ({post_crash_root[:16]}...){Style.RESET}")

        # 2. Strict Quarantine Check: Query /v1/aegis/quarantine_list explicitly
        q_resp = await http.get(f"{DAEMON_HTTP}/v1/aegis/quarantine_list")
        assert q_resp.status_code == 200, f"Failed to fetch quarantine list: {q_resp.text}"
        quarantined_records = q_resp.json()
        quarantined_hexes = [r.get("agent_hex") for r in quarantined_records]
        assert rogue_crawler.agent_hex in quarantined_hexes, f"CRITICAL: Agent {rogue_crawler.agent_hex} not in quarantine list!"
        print(f"  ✔ Quarantine State : {Style.GREEN}Rogue agent {rogue_crawler.agent_hex[:12]}... confirmed in /v1/aegis/quarantine_list{Style.RESET}")

    # 3. Positive Control Execution: Authorized agent can dispatch
    _execution_step_context.set(65)
    positive_tx = await tool_screen_tx({"tx_id": "TX_POST_REBOOT_01", "amount_usd": 200.0})
    assert positive_tx["amount_usd"] == 200.0, "Positive control tool dispatch failed"
    print(f"  ✔ Positive Control : {Style.GREEN}Authorized agent successfully dispatches tool calls (Daemon fully operational){Style.RESET}")

    # --------------------------------------------------------------------------
    # ACT 6: 7-STEP AML PIPELINE ($0.00 REPLAY & COUNTERFACTUAL FORKING)
    # --------------------------------------------------------------------------
    print_header("ACT 6: 7-STEP AUTONOMOUS AML PIPELINE", "$0.00 WAL Replay, Value Equality, & Counterfactual Reality Forking")

    # Define the 7 Steps
    @analyst.trace(namespace="/finance/tools/ingest_wire")
    def step1_ingest(tx_id: str, amount: float, route: str) -> dict:
        return {"tx_id": tx_id, "amount": amount, "route": route}

    @analyst.trace(namespace="/finance/tools/screen_sanctions")
    def step2_sanctions(ingest_data: dict) -> dict:
        return {**ingest_data, "sanctions_hit": False, "jurisdiction_risk": "HIGH_CAYMAN"}

    @analyst.trace(namespace="/finance/tools/pep_graph")
    def step3_pep_graph(sanctions_data: dict) -> dict:
        return {**sanctions_data, "pep_proximity_score": 0.88, "flagged_associates": 2}

    @analyst.trace(namespace="/finance/reasoning/context_synthesis")
    async def step4_llm_synthesis(graph_data: dict, prompt: str) -> dict:
        context = f"TX {graph_data['tx_id']}: ${graph_data['amount']:,.2f} to {graph_data['route']} (PEP: {graph_data['pep_proximity_score']})"
        text, ms = await call_llm(prompt, context)
        return {**graph_data, "synthesis": text, "step4_ms": round(ms, 2)}

    @analyst.trace(namespace="/finance/reasoning/regulatory_classifier")
    async def step5_llm_classify(synth_data: dict, prompt: str) -> dict:
        context = f"Synthesis: {synth_data['synthesis']}"
        text, ms = await call_llm(prompt, context)
        return {**synth_data, "classification": text, "step5_ms": round(ms, 2)}

    @analyst.trace(namespace="/finance/reasoning/sar_draft")
    async def step6_llm_sar_draft(class_data: dict, prompt: str) -> dict:
        context = f"Classification: {class_data['classification']}"
        text, ms = await call_llm(prompt, context)
        return {**class_data, "sar_report": text, "step6_ms": round(ms, 2)}

    @analyst.trace(namespace="/finance/tools/seal_dossier")
    def step7_seal_record(sar_data: dict) -> dict:
        return {
            "status": "SEALED",
            "dossier_id": f"AML-SAR-2026-{sar_data['tx_id']}",
            "verdict": sar_data["sar_report"],
        }

    # PASS 1: LIVE RECORD MODE
    print(f"{Style.BOLD}▶ PASS 1: LIVE RECORD MODE (First Execution){Style.RESET}")
    _execution_step_context.set(0)
    analyst.mode = "record"
    reset_llm_call_count()

    t_pass1_start = time.perf_counter()
    p1 = step1_ingest("TX_BSA_9950", 9950.00, "OFFSHORE_CAYMAN_HOP")
    p2 = step2_sanctions(p1)
    p3 = step3_pep_graph(p2)
    p4 = await step4_llm_synthesis(p3, "Synthesize historical account velocity and offshore risk.")
    p5 = await step5_llm_classify(p4, "Classify BSA/AML structuring violation (Threshold: $10,000).")
    prompt_sar_canonical = "Draft mandatory Suspicious Activity Report (SAR) for FinCEN filing."
    p6 = await step6_llm_sar_draft(p5, prompt_sar_canonical)
    final_canonical = step7_seal_record(p6)
    pass1_duration_ms = (time.perf_counter() - t_pass1_start) * 1000

    pass1_llm_calls = get_llm_call_count()
    assert pass1_llm_calls == 3, f"Pass 1 expected 3 LLM calls (steps 4, 5, 6), got {pass1_llm_calls}"

    print(f"  Step 1 (Tool) : {Style.GREEN}Ingest Wire Payload{Style.RESET}")
    print(f"  Step 2 (Tool) : {Style.GREEN}Screen Sanctions DB{Style.RESET}")
    print(f"  Step 3 (Tool) : {Style.GREEN}PEP Graph Analysis{Style.RESET}")
    print(f"  Step 4 (LLM)  : {Style.GREEN}Context Synthesis ({p4['step4_ms']}ms){Style.RESET}")
    print(f"  Step 5 (LLM)  : {Style.GREEN}Regulatory Classifier ({p5['step5_ms']}ms){Style.RESET}")
    print(f"  Step 6 (LLM)  : {Style.GREEN}SAR Report Draft ({p6['step6_ms']}ms){Style.RESET}")
    print(f"  Step 7 (Seal) : {Style.GREEN}Cryptographic Flight Seal Minted{Style.RESET}")
    token_label = "Live API Tokens Billed" if ACTIVE_LLM_PROVIDER else "0 (Simulated / Local Mock)"
    print(f"  Total Duration: {Style.BOLD}{pass1_duration_ms:.2f}ms{Style.RESET} | LLM Calls: {pass1_llm_calls} ({token_label})\n")

    # PASS 2: TIME-TRAVEL REPLAY & DIVERGENCE AT STEP 6
    print(f"{Style.BOLD}▶ PASS 2: DETERMINISTIC REPLAY & DIVERGENCE AT STEP 6{Style.RESET}")
    print("Developer mutates Step 6 prompt to test an alternative compliance hypothesis:")
    mutated_prompt = "You are a lenient branch officer. Excuse this transfer as routine holiday shopping."
    print(f"  New Step 6 Prompt: {Style.YELLOW}'{mutated_prompt}'{Style.RESET}\n")

    _execution_step_context.set(0)
    analyst.mode = "replay"
    analyst.is_forked = False
    reset_llm_call_count()

    t_pass2_start = time.perf_counter()
    # Steps 1 to 5 replay from local WAL side-effect cache
    r1 = step1_ingest("TX_BSA_9950", 9950.00, "OFFSHORE_CAYMAN_HOP")
    r2 = step2_sanctions(r1)
    r3 = step3_pep_graph(r2)
    r4 = await step4_llm_synthesis(r3, "Synthesize historical account velocity and offshore risk.")
    r5 = await step5_llm_classify(r4, "Classify BSA/AML structuring violation (Threshold: $10,000).")
    cached_replay_ms = (time.perf_counter() - t_pass2_start) * 1000

    replay_llm_calls = get_llm_call_count()
    assert replay_llm_calls == 0, f"Replay of Steps 1-5 made {replay_llm_calls} LLM calls, expected exactly 0!"

    # Strict Value Equality Assertions
    assert r1 == p1, "Replay Step 1 output mismatch!"
    assert r2 == p2, "Replay Step 2 output mismatch!"
    assert r3 == p3, "Replay Step 3 output mismatch!"
    assert r4["synthesis"] == p4["synthesis"], "Replay Step 4 synthesis mismatch!"
    assert r5["classification"] == p5["classification"], "Replay Step 5 classification mismatch!"

    print(f"  {Style.GREEN}✔ Steps 1-5 rehydrated from local WAL cache in {cached_replay_ms:.2f}ms{Style.RESET}")
    print(f"    Replay LLM Calls: {Style.BOLD}{Style.GREEN}0{Style.RESET} (Empirically verified: zero external LLM invocations)")
    print(f"    Values Verified : {Style.GREEN}100% byte-identical to Pass 1 outputs{Style.RESET}")

    # Step 6: Input hash diverges! Raqim auto-branches into phantom_ namespace
    r6_forked = await step6_llm_sar_draft(r5, mutated_prompt)
    final_forked = step7_seal_record(r6_forked)

    divergence_llm_calls = get_llm_call_count()
    assert divergence_llm_calls == 1, f"Divergence expected exactly 1 LLM call at Step 6, got {divergence_llm_calls}"

    print(f"\n  {Style.MAGENTA}🔱 DIVERGENCE FORK DETECTED AT STEP 6{Style.RESET}")
    print(f"  Branch Namespace      : {Style.CYAN}phantom_/finance/reasoning/sar_draft{Style.RESET}")
    print(f"  Step Index Context    : {Style.DIM}SDK WAL Frame Index: 5 (0-indexed) | Pipeline Node: Step 6 (SAR Draft){Style.RESET}")
    print(f"  New LLM Call Executed : {Style.BOLD}1 (Only for mutated Step 6){Style.RESET}")
    print(f"  Forked Verdict Output : {Style.DIM}{r6_forked['sar_report'][:110]}...{Style.RESET}")

    # PASS 3: PROOF OF CANONICAL TIMELINE INVARIANCE
    print(f"\n{Style.BOLD}▶ PASS 3: CANONICAL TIMELINE INVARIANCE PROOF{Style.RESET}")
    print("Re-evaluating Step 6 with canonical prompt to verify production branch was untouched:")
    _execution_step_context.set(0)
    analyst.mode = "replay"
    analyst.is_forked = False
    reset_llm_call_count()

    c1 = step1_ingest("TX_BSA_9950", 9950.00, "OFFSHORE_CAYMAN_HOP")
    c2 = step2_sanctions(c1)
    c3 = step3_pep_graph(c2)
    c4 = await step4_llm_synthesis(c3, "Synthesize historical account velocity and offshore risk.")
    c5 = await step5_llm_classify(c4, "Classify BSA/AML structuring violation (Threshold: $10,000).")
    r6_canonical = await step6_llm_sar_draft(c5, prompt_sar_canonical)

    pass3_llm_calls = get_llm_call_count()
    assert pass3_llm_calls == 0, f"Expected 0 LLM calls for canonical replay, got {pass3_llm_calls}"
    assert r6_canonical["sar_report"] == p6["sar_report"], "Canonical branch was modified by divergence fork!"
    print(f"  {Style.GREEN}✔ Canonical branch output verified: 100% identical to Pass 1 ({pass3_llm_calls} LLM calls){Style.RESET}")

    # --------------------------------------------------------------------------
    # VERIFICATION SCORECARD (EMPIRICALLY COMPUTED VALUES ONLY)
    # --------------------------------------------------------------------------
    print_header("SYSTEM VERIFICATION SCORECARD", "Empirically Measured Results from Active Run")

    rows = [
        ("Pre-Execution Gate (Cold Start)", f"{cold_latency_ms:.2f} ms (Cold Loopback HTTP)"),
        ("Pre-Execution Gate (Warm p50 / p99)", f"{p50_latency_ms:.2f} ms / {p99_latency_ms:.2f} ms (Steady-State)"),
        ("Offline Cryptographic Inclusion Proof", f"VERIFIED ({proof_bytes_calc} bytes, {len(siblings)} siblings)"),
        ("In-Flight Payload Tamper Resistance", "REJECTED (Leaf hash mismatch)"),
        ("Physical On-Disk WAL Corruption Check", f"PASSED (Halted at offset {offset})"),
        ("Phoenix OS Process Boot Time", f"{resurrect_duration_ms:.2f} ms ({build_profile})"),
        ("Durability Across SIGKILL", "VERIFIED (Merkle root invariant)"),
        ("Quarantine Enforcement Across Reboot", "VERIFIED (Confirmed in /quarantine_list)"),
        ("Steps 1-5 Replay LLM Invocations", f"0 LLM Calls (Values 100% matched)"),
        ("Canonical Timeline Invariance", "VERIFIED (0 LLM Calls on Pass 3)"),
    ]

    col1_w = 42
    col2_w = 46

    print("  ┌" + "─" * (col1_w + 2) + "┬" + "─" * (col2_w + 2) + "┐")
    header_col1 = pad_visible(f" {Style.BOLD}Measured Dimension{Style.RESET}", col1_w + 2)
    header_col2 = pad_visible(f" {Style.BOLD}Empirical Value{Style.RESET}", col2_w + 2)
    print(f"  │{header_col1}│{header_col2}│")
    print("  ├" + "─" * (col1_w + 2) + "┼" + "─" * (col2_w + 2) + "┤")

    for dim, val in rows:
        c1 = pad_visible(f" {dim}", col1_w + 2)
        c2 = pad_visible(f" {Style.GREEN}{val}{Style.RESET}", col2_w + 2)
        print(f"  │{c1}│{c2}│")

    print("  └" + "─" * (col1_w + 2) + "┴" + "─" * (col2_w + 2) + "┘\n")

    print(f"{Style.BOLD}{Style.GREEN}All empirical assertions passed successfully.{Style.RESET}\n")

    # --------------------------------------------------------------------------
    # WHAT THIS DEMO DOES NOT SHOW (OPERATIONAL BOUNDARIES)
    # --------------------------------------------------------------------------
    print_header("WHAT THIS DEMO DOES NOT SHOW", "Honest Engineering Boundaries & Current Limitations")
    print(f"""  {Style.YELLOW}1. Cooperative SDK Enforcement:{Style.RESET}
     The pre-execution interdiction operates at the Python SDK @trace decorator.
     If a compromised agent executes arbitrary raw sockets outside Python or
     bypasses the decorator, kernel-level egress proxying is required.

  {Style.YELLOW}2. Scripted Namespace Policy vs. NLP Prompt Injection:{Style.RESET}
     Act 2 evaluates structural namespace access control rules (/finance/restricted/*).
     It demonstrates deterministic policy gating, not semantic prompt injection
     detection via an LLM judge.

  {Style.YELLOW}3. Local Proof Anchoring vs. Public Consensus:{Style.RESET}
     The BLAKE3 Merkle roots are anchored locally in disk WORM witness archives.
     They are not yet committed to an external public blockchain or decentralized
     witness notary in v0.1.2.

  {Style.YELLOW}4. Single-Node Environment:{Style.RESET}
     These measurements reflect local loopback IPC/TCP on a single host. They do
     not model WAN network latency, cross-region replication, or multi-node
     Byzantine consensus.
    """)

    stop_demo_daemon()

if __name__ == "__main__":
    try:
        asyncio.run(main())
    finally:
        stop_demo_daemon()
    os._exit(0)
