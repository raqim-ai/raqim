import asyncio
import base64
import contextvars
import functools
import inspect
import json
import uuid
from typing import (
    Any,
    AsyncGenerator,
    Awaitable,
    Callable,
    Dict,
    List,
    Optional,
    Tuple,
)

import blake3
import httpx
import concurrent.futures
import websockets
import zenoh

from raqim_core import RaqimCryptoCore  

# ASYNC CONTEXT PROPAGATION & ERROR SCHEMAS

class ReplayDivergedError(Exception): 
    """Raised when replay Python code diverges from recorded WAL history."""
    pass

class RaqimClientError(Exception): 
    """Raised for general Raqim client communication or cryptographic errors."""
    pass
    
# Task-Local context tracker 
_execution_step_context: contextvars.ContextVar[int] = contextvars.ContextVar("raqim_step_context", default = 0)

# 2. CANONICAL ARGUMENTT SERIALIZER

class CanonicalSerializer: 
    """
    Normalizes arbitrary Python objects, Pydantic models, and function arguments into a deterministic, 
    canonical JSON byte representation for BLAKE3 hashing.
    """
    
    @staticmethod
    def _default_encoder(obj: Any) -> Any: 
        # pydantic v2 support
        if hasattr(obj, "model_dump"): 
            return obj.model_dump()
        # Pydantic v1 support
        if hasattr(obj, "dict"): 
            return obj.dict()
        # Dataclass support
        if hasattr(obj, "__dataclass_fields__"): 
            import dataclasses
            return dataclasses.asdict(obj)
        # Byte support
        if isinstance(obj, (bytes, bytearray)): 
            return base64.b16encode(obj).decode("ascii")
        
        # Fallback to string representation
        return str(obj)
    
    @classmethod
    def canonical_json(cls, data: Any) -> str: 
        return json.dumps(data, default=cls._default_encoder, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        
    @classmethod
    def derive_call_signature_hash(cls, fn: Callable[..., Any], args: Tuple[Any, ...], kwargs: Dict[str, Any], custom_signature: Optional[str] = None ) -> Tuple[str, str]: 
        """ 
        Extracts function signature, binds parameters with defaults, and computes a 
        domain-separated 32-byte BLAKE3 hash.
        """
        # Bind arguments to formal parameters and indect defaults
        sig = inspect.signature(fn)
        bound_args = sig.bind(*args, **kwargs)
        bound_args.apply_defaults()
    
        # Build normalized call directory
        func_name = custom_signature or f"{fn.__module__}.{fn.__qualname__}"
        normalized_payload = {
            "function": func_name,
            "arguments": bound_args.arguments, 
        }
        
        # Serialize to deterministic Canonical JSON
        canonical_str = cls.canonical_json(normalized_payload)
        
        # 4. Compute 32-byte BLAKE3 Hash using Domain Separation Key
        hasher = blake3.blake3(derive_key_context="raqim.effect.v1.signature")
        hasher.update(canonical_str.encode("utf-8"))
        call_sig_hash = hasher.digest(length=32)
        
        return call_sig_hash.hex(), canonical_str
        

class RaqimClient:
    def __init__(
        self, alias: str, tenant: str, private_key_path: str, cert_path: Optional[str] = None,
        daemon_host: str = "127.0.0.1", tcp_port: int = 8080, http_port: int = 8081, 
        mode: str = "record", # "record" (Live) or "reply" (Deterministic Time-Travel)
        on_divergence: str = "fork" # "fork" (Auto-branch into phantom namespace) or "raise" (scream out the divergence)
        ):
        self.alias = alias 
        self.tenant = tenant 
        self.crypto_core = RaqimCryptoCore(private_key_path, cert_path)
        self.cert_bytes = bytes(self.crypto_core.capability_cert_bytes)
        self.cert_hex = self.cert_bytes.hex()

       # Mathematically derive 16-byte Agent ID via Blake3 Domain Separation
        public_key_bytes = bytes(self.crypto_core.pub_key_bytes)
        derived_16_bytes = blake3.blake3(public_key_bytes, derive_key_context="raqim.agent.v1.identity").digest(length=16)
       
        # The 32-character hex string representing the 16-bytes
        self.agent_hex = derived_16_bytes.hex()
        self.tcp_addr = (daemon_host, tcp_port)
        self.http_url = f"http://{daemon_host}:{http_port}"
        self.ws_url = f"ws://{daemon_host}:{http_port}/v1/mcp/ws"
      
        self.mode = mode
        self.on_divergence = on_divergence
        self.is_forked = False

        # THE ASYNC MULTIPLEXER (Python's equivalent to DashMap + oneshot)
        self._pending_requests: Dict[str, asyncio.Future] = {}
        self._capabilities: Dict[str, Callable[[bytes], Awaitable[bytes]]] = {}
        self._ws_connection: Optional[websockets.WebSocketClientProtocol] = None
        self._zenoh_session: Optional[Any] = None
        # The callback function provided by the developer
        self._reality_fork_hook: Callable[[str], None] = None 
    
    async def boot(self): 
        """
        Enterprise Ignition Sequence: 
        - Emits /system/handshake over TCPP to trigger JIT CRDT memory hydration
        - Mounts zenoh control subscriber for Aegis out-of-band context eviction
        """
        # 1. TCP Handshake Protocol (Registers Alias with RAM Process Table)
        await self.commit_thought(
            agent_hex=self.agent_hex,
            intent_path="/system/handshake",
            text=f"ALIAS={self.alias}"
        )
        print(f"[BOOT] Agent '{self.alias}' ({self.agent_hex[:8]}...) registered.")

        # 2. Establish Zenoh Control Plane for Aegis Circuit Breakers
        try:
            self._zenoh_session = zenoh.open(zenoh.Config())
            control_topic = f"raqim/{self.tenant}/control/{self.agent_hex}"
            self._zenoh_session.declare_subscriber(control_topic, self._handle_os_control_override)
        except Exception as e: 
            print(f"[BOOT WARN] Zenoh control plane unavailable: {e}. Running in local-only mode.")

    def register_eviction_hook(self, callback: Callable[[str], None]): 
        """
            Registers the developer callback for Aegis FORCE_CONTEXT_EVICTION events.
        """
        self._reality_fork_hook = callback

    def _handle_os_control_override(self, sample: Any) -> None:
        """Listener that wipes corrupted context when Aegis trips a circuit breaker """
        
        try:
            payload = json.loads(sample.payload.decode('utf-8'))
            if payload.get("command") == "FORCE_CONTEXT_EVICTION":
                print(f"\n[OS RED ALERT] Aegis Firewall mandated a Reality Re-seed.")
                new_system_prompt = payload.get("new_system_prompt", "")
                print(f"[OS DIRECTIVE]: {new_system_prompt}")
                # Trigger the closure
                if self._reality_fork_hook: 
                    self._reality_fork_hook(new_system_prompt)
                    print("[OS OVERRIDE] Developer hook executed. Reality re-seeded.")
                else: 
                    print("[OS WARNING] No eviction hook registered. Context may be corrupted ")
        except Exception as e: 
            print(f"[OS ERROR] Failed to process control overrides: {e} ", e)

# LOW-LEVEL DATA PLANE & RAG QUERIES
    async def commit_thought(self, agent_hex: str, intent_path: str, text: str) -> None:
        """Shoots signed zero-copy rkyv bytes over raw TCP to Raqim's WAL Engine"""
        # The Rust PyO3 extension handles the blazing-fast serialization and signing
        raw_payload = self.crypto_core.generate_tcp_payload(agent_hex, intent_path, text)
        
        reader, writer = await asyncio.open_connection(*self.tcp_addr)
        try: 
            writer.write(raw_payload)
            await writer.drain()
        finally:
            writer.close()
            await writer.wait_closed()

    async def query_memory(self, intent_path: str, query: str, limit: int = 5) -> list[str]:
        """Queries Raqim's Unified Hybrid Search Router (LanceDB + Hot RAM Buffer)."""
        async with httpx.AsyncClient() as client:
            resp = await client.get(
                f"{self.http_url}/v1/swarm/memory",
                params={"namespace": intent_path, "query": query, "limit": limit},
            )
            resp.raise_for_status()
            return resp.json()

    # @raqim.trace DECORATOR 
    def trace(self, namespace: str = "/default", custom_signature: Optional[str] = None) -> Callable[..., Any]: 
        """ 
        @raqim.trace Decorator: 
        Wraps any sync function, async coroutine, or async streaming generator. 
        - In 'record' mode: Runs fn live, records result to Raqim WAL. 
        - In 'replay' mode: Bypasses execution, fetches output from WAL ($0 API cost). 
        - On code change: Auto-forks execution into a parallel universe branch.
        """
        def decorator(fn: Callable[..., Any]) -> Callable[..., Any]: 
            if inspect.isasyncgenfunction(fn): 
                # Path A: Async Generator (Streaming LLM tokens)
                @functools.wraps(fn)
                async def async_gen_wrapper(*args: Any, **kwargs: Any) -> AsyncGenerator[Any, None]:
                    step = _execution_step_context.get()
                    _execution_step_context.set(step + 1)
                    
                    call_sig_hex, _ = CanonicalSerializer.derive_call_signature_hash(fn, args, kwargs, custom_signature)
                    
                    target_ns = namespace
                    if self.is_forked: 
                        target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"

                    # Replay Check
                    if self.mode == "replay" and not self.is_forked: 
                        cached = await self._fetch_recorded_effect(step, call_sig_hex)
                        if cached is not None: 
                            print(f"[RAQIM REPLAY] Step {step} (Stream) replayed from WAL ($0 API cost).")
                            for chunk in cached: 
                                yield chunk
                            return 
                        
                        # Divergence Triggered
                        self._handle_divergence(step, call_sig_hex, namespace)
                        target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"

                    # Live Execution & accumulation
                    accumulated_chunks: List[Any] = []
                    async for item in fn(*args, **kwargs):
                        accumulated_chunks.append(item)
                        yield item 
                    
                    # Persist accumulated stream to WAL
                    await self._persist_effect(step, call_sig_hex, accumulated_chunks, target_ns)
                
                return async_gen_wrapper
            
            elif asyncio.iscoroutinefunction(fn): 
                # Path B: Standard Async Coroutine
                @functools.wraps(fn) 
                async def async_wrapper(*args: Any, **kwargs: Any) -> Any: 
                    step = _execution_step_context.get()
                    _execution_step_context.set(step + 1)
                    
                    call_sig_hex, _ = CanonicalSerializer.derive_call_signature_hash(fn, args, kwargs, custom_signature)
                    
                    target_ns = namespace
                    if self.is_forked: 
                        target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"
                    
                    # Replay Check
                    if self.mode == "replay" and not self.is_forked: 
                        cached = await self._fetch_recorded_effect(step, call_sig_hex)
                        if cached is not None: 
                            print(f"[RAQIM REPLAY] Step {step} replayed from WAL ($0 API cost).")
                            return cached
                        
                        # Divergence Triggered
                        self._handle_divergence(step, call_sig_hex, namespace)
                        target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"
                    
                    # Live execution
                    result = await fn(*args, **kwargs) 
                    await self._persist_effect(step, call_sig_hex, result, target_ns)
                    return result
                
                return async_wrapper
            
            else: 
                # Path C: Synchronous function
                @functools.wraps(fn)
                def sync_wrapper(*args: Any, **kwargs: Any) -> Any: 
                    step = _execution_step_context.get()
                    _execution_step_context.set(step + 1)
                    
                    call_sig_hex, _ = CanonicalSerializer.derive_call_signature_hash(fn, args, kwargs, custom_signature)
                    
                    target_ns = namespace
                    if self.is_forked:
                        target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"
                        
                    # For sync functions, bridge to async execution via loop runner
                    try: 
                        running_loop = asyncio.get_running_loop()
                    except: 
                        running_loop = None 
                    if running_loop and running_loop.is_running():
                        # Scenario 1: Sync function called inside active async loop (e.g. inside main())
                        if self.mode == "replay" and not self.is_forked: 
                            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                                cached = pool.submit(lambda: asyncio.run(self._fetch_recorded_effect(step, call_sig_hex))).result()
                            
                            if cached is not None: 
                                print(f"[RAQIM REPLAY] Step {step} (Sync) replayed from WAL ($0 API cost).")
                                return cached 

                            self._handle_divergence(step, call_sig_hex, namespace)
                            target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"
                        
                        result = fn(*args, **kwargs)
                        # Schedule persistence without blocking the running loop
                        asyncio.create_task(self._persist_effect(step, call_sig_hex, result, target_ns))
                        return result
                    else:
                        # Scenario 2: Sync function called from standard synchronous context
                        loop = self._get_or_create_event_loop()
                        
                        if self.mode == "replay" and not self.is_forked: 
                            cached = loop.run_until_complete(
                                self._fetch_recorded_effect(step, call_sig_hex)
                            ) 
                            if cached is not None: 
                                print(f"[RAQIM REPLAY] Step {step} (Sync) replayed from WAL ($0 API cost).")
                                return cached 

                            self._handle_divergence(step, call_sig_hex, namespace)
                            target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"
                        
                        result = fn(*args, **kwargs)
                        loop.run_until_complete(self._persist_effect(step, call_sig_hex, result, target_ns))
                        return result 
                
                return sync_wrapper

        return decorator
   
    # Internal Effect Engine Helpers
    async def _fetch_recorded_effect(self, step_ordinal: int, call_sig_hex  : str) -> Optional[Any]:
        """Fetches recorded effect from daemon. Returns None if signature diverged."""
        async with httpx.AsyncClient() as http: 
            try: 
                res = await http.post(
                    f"{self.http_url}/v1/effect/get", 
                    json={"agent_hex": self.agent_hex, "step_ordinal": step_ordinal, "call_signature_hex": call_sig_hex }, 
                    timeout=5.0
                )
                
                if res.status_code == 200:
                    data = res.json()
                    if data.get("found") and data.get("output_payload_base64"): 
                        raw_bytes = base64.b64decode(data["output_payload_base64"])
                        return json.loads(raw_bytes.decode("utf-8"))
            except Exception as e: 
                print(f"[RAQIM REPLAY WARN] Effect fetch error at step {step_ordinal}: {e}")
        return None

    async def _persist_effect(self, step_ordinal: int, call_signature_hex: str, result: Any, namespace: str) -> None: 
        """Persists live execution output into Raqim's WAL + Merkle DAG."""
        canonical_output = CanonicalSerializer.canonical_json(result)
        b64_output = base64.b64encode(canonical_output.encode("utf-8")).decode("ascii")
        
        async with httpx.AsyncClient() as http: 
            try: 
                await http.post(
                    f"{self.http_url}/v1/effect/record", 
                    json={
                        "agent_hex": self.agent_hex, 
                        "step_ordinal": step_ordinal, 
                        "call_signature_hex": call_signature_hex, 
                        "namespace": namespace,
                        "output_payload_base64": b64_output 
                    }, 
                    timeout=5.0
                )
                if self.is_forked: 
                    print(f"[RAQIM FORM RECORD] Step {step_ordinal} recorded to branch: {namespace}")
            except Exception as e: 
                print(f"[RAQIM RECORD ERROR] Failed to persist effect at step {step_ordinal}: {e}")

    def _handle_divergence(self, step: int, call_sig_hex: str, namespace: str) -> None: 
        """Executes the divergence policy when replayed code does not match WAL history.""" 
        if self.on_divergence == "raise":
            raise ReplayDivergedError(
                                      f"[RAQIM REPLAY DIVERGED] code modified at Step {step} " 
                                      f"(Signature: {call_sig_hex[:8]}...). No recorded trace matches history."
                                      )
        
        self.is_forked = True 
        phantom_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"
        print(
            f"\n [RAQIM PARALLEL UNIVERSE FORK] Code divergence at Step {step}! "
            f"Auto-swtiching REPLAY -> LIVE mode on branch: {phantom_ns}"
        )
        
    def _get_or_create_event_loop(self) -> asyncio.AbstractEventLoop: 
        try: 
            loop = asyncio.get_event_loop() 
            if loop.is_closed(): 
                loop = asyncio.new_event_loop()
                asyncio.set_event_loop(loop)
            return loop 
        except RuntimeError: 
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            return loop
    
    # A2A Websocket Swarm router 
    async def connect_swarm(self):
        """Connect  background WebSocket Multiplexer to Raqim's A2A gateway"""
        self._ws_connection = await websockets.connect(self.ws_url)
        asyncio.create_task(self._websocket_listener())

    async def _websocket_listener(self):
        """The Background Router matching incoming A2A tp suspended Futures"""
        if not self._ws_connection: 
            return 
        try:
            async for message in self._ws_connection:
                data = json.loads(message)
                msg_type = data.get("type")

                if msg_type == "QuestionAnswered":
                    # We got an answer! Wake up the specific suspended function.
                    req_id = data["request_id"]
                    if req_id in self._pending_requests:
                        future = self._pending_requests.pop(req_id)
                        future.set_result(data.get("answer")) 
                
                elif msg_type == "IncomingQuestion":
                    # Someone is asking us a question!
                    cap = data.get("capability")
                    if cap and cap in self._capabilities:
                        handler = self._capabilities[cap]
                        # Execute the user's AI logic
                        answer_bytes = await handler(bytes(data.get("question")))
                        
                        # Send the reply back up the socket
                        reply = {
                            "type": "ReplyToQuestion",
                            "request_id": data.get("request_id"),
                            "answer": list(answer_bytes), 
                            "responder_hex": self.agent_hex
                        }
                        await self._ws_connection.send(json.dumps(reply))
                        
        except websockets.ConnectionClosed:
            print("[RAQIM] Swarm WebSocket connection closed.")

    async def ask_swarm(self, capability: str, question: bytes, sender_hex: str) -> bytes:
        """Suspends the Python coroutine until the answer arrives over A2A network"""
        if not self._ws_connection:
            raise RaqimClientError("Must call `await client.connect_swarm()` before querying swarm.")

        request_id = str(uuid.uuid4())
        loop = asyncio.get_running_loop()
        future: asyncio.Future[bytes] = loop.create_future()
        self._pending_requests[request_id] = future

        # True crytography
        signature = self.crypto_core.sign_payload(question)

        ask_msg = {
            "type": "AskQuestion",
            "request_id": request_id,
            "capability": capability,
            "question": list(question),
            "sender_hex": sender_hex,
            "public_key": list(self.crypto_core.pub_key_bytes),
            "signature": list(signature),
            "capability_cert": self.cert_hex
        }

        await self._ws_connection.send(json.dumps(ask_msg))
        
        # ZERO CPU YIELD: Suspends Python execution until the _websocket_listener wakes it up
        return await asyncio.wait_for(future, timeout=15.0)

    async def serve_capability(self, capability: str, handler: Callable[[bytes], Awaitable[bytes]]):
        """Exposes an AI logic capability to the global swarm."""
        if not self._ws_connection:
            raise RaqimClientError("Must call `await client.connect_swarm() first.")
            
        self._capabilities[capability] = handler
        msg = {"type": "RegisterCapability", "capability": capability}
        await self._ws_connection.send(json.dumps(msg))

# Zero Dependency Offline merkle proof verifier
def verify_state_proof_offline(payload_bytes: bytes, agent_id_str: str, proof_dict: dict) -> bool: 
    """ 
    OFFLINE MERKLE VERIFIER 
    Recomputes the Blake3 Merkle path offline with ZERO networkk calls. 
    And as expected returns True if the transaction is mathematically bound to the Public Merkle root
    """
    agent_id_bytes = bytes.fromhex(agent_id_str)
    
    # Recompute leaf hash 
    hasher = blake3.blake3(derive_key_context="raqim.axon.v1.leaf")
    hasher.update(payload_bytes)
    hasher.update(agent_id_bytes)
    current_hash = hasher.digest(length=32)
    
    index = proof_dict["leaf_index"]
    
    # Recompute path up the binary tre 
    for sibling_hex in proof_dict["sibling_hashes_hex"]: 
        sibling_bytes = bytes.fromhex(sibling_hex) 
        
        node_hasher = blake3.blake3(derive_key_context="raqim.axon.v1.node")
        if index % 2 == 0: 
            node_hasher.update(current_hash)
            node_hasher.update(sibling_bytes)
        else: 
            node_hasher.update(sibling_bytes)
            node_hasher.update(current_hash)
        
        current_hash = node_hasher.digest(length = 32)
        index //=2
        
    return current_hash.hex() == proof_dict["merkle_root_hex"]

