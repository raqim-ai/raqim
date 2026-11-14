import asyncio
import base64
import concurrent.futures
import contextlib
import contextvars
import functools
import inspect
import json
import time
from typing import (
    Any,
    AsyncGenerator,
    Callable,
    Dict,
    List,
    Optional,
    Tuple,
)

import blake3
import httpx

from raqim_core import RaqimCryptoCore


class ReplayDivergedError(Exception):
    """Raised when replayed Python code diverges from recorded WAL history under on_divergence='raise'."""
    pass


class RaqimClientError(Exception):
    """Raised for general Raqim client communication or cryptographic errors."""
    pass


# Task-local step ordinal counter
_execution_step_context: contextvars.ContextVar[int] = contextvars.ContextVar(
    "raqim_step_context", default=0
)


class CanonicalSerializer:
    """
    Normalizes arbitrary Python objects, Pydantic models, dataclasses, and function arguments
    into a deterministic, key-sorted JSON byte representation for BLAKE3 hashing.
    """

    @staticmethod
    def _default_encoder(obj: Any) -> Any:
        if hasattr(obj, "model_dump"):
            return obj.model_dump()
        if hasattr(obj, "dict"):
            return obj.dict()
        if hasattr(obj, "__dataclass_fields__"):
            import dataclasses
            return dataclasses.asdict(obj)
        if isinstance(obj, (bytes, bytearray)):
            return base64.b16encode(obj).decode("ascii")
        return str(obj)

    @classmethod
    def canonical_json(cls, data: Any) -> str:
        return json.dumps(
            data,
            default=cls._default_encoder,
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=False,
        )

    @classmethod
    def derive_call_signature_hash(
        cls,
        fn: Callable[..., Any],
        args: Tuple[Any, ...],
        kwargs: Dict[str, Any],
        custom_signature: Optional[str] = None,
    ) -> Tuple[str, str]:
        sig = inspect.signature(fn)
        bound_args = sig.bind(*args, **kwargs)
        bound_args.apply_defaults()

        func_name = custom_signature or f"{fn.__module__}.{fn.__qualname__}"
        normalized_payload = {
            "function": func_name,
            "arguments": bound_args.arguments,
        }

        canonical_str = cls.canonical_json(normalized_payload)
        hasher = blake3.blake3(derive_key_context="raqim.effect.v1.signature")
        hasher.update(canonical_str.encode("utf-8"))
        call_sig_hash = hasher.digest(length=32)

        return call_sig_hash.hex(), canonical_str


class TraceMetadata:
    """Standard container for step-level execution and model token telemetry."""

    def __init__(
        self,
        system: Optional[str] = None,
        model: Optional[str] = None,
        prompt_tokens: Optional[int] = None,
        completion_tokens: Optional[int] = None,
        custom_attributes: Optional[Dict[str, Any]] = None,
    ):
        self.system = system
        self.model = model
        self.prompt_tokens = prompt_tokens
        self.completion_tokens = completion_tokens
        self.custom_attributes = custom_attributes or {}

    @classmethod
    def from_response(cls, response: Any, default_model: Optional[str] = None) -> "TraceMetadata":
        meta = cls(model=default_model)
        if not response:
            return meta

        if isinstance(response, dict):
            usage = response.get("usage") or response.get("usageMetadata") or {}
            meta.prompt_tokens = usage.get("prompt_tokens") or usage.get("promptTokenCount")
            meta.completion_tokens = usage.get("completion_tokens") or usage.get("candidatesTokenCount")
            meta.model = response.get("model") or default_model
            return meta

        usage_obj = getattr(response, "usage", None) or getattr(response, "usage_metadata", None)
        if usage_obj:
            meta.prompt_tokens = getattr(usage_obj, "prompt_tokens", None) or getattr(
                usage_obj, "prompt_token_count", None
            )
            meta.completion_tokens = getattr(usage_obj, "completion_tokens", None) or getattr(
                usage_obj, "candidates_token_count", None
            )
            meta.model = getattr(response, "model", None) or default_model

        return meta


class RaqimClient:
    """
    Raqim v0.2.0 Execution-Integrity & Zero-Secret Gateway Client.
    Supports both:
      1. Zero-Secret HTTP Egress Proxying (`/v1/egress/{connector}/{path}`) via `rqm_v1` tokens.
      2. Step-level local function tracing, $0.00 deterministic replay, and counterfactual forking (`@client.trace`).
    """

    def __init__(
        self,
        alias: str,
        tenant: str,
        private_key_path: str,
        cert_path: Optional[str] = None,
        workload_token: Optional[str] = None,
        daemon_host: str = "127.0.0.1",
        tcp_port: int = 8080,
        http_port: int = 8081,
        mode: str = "record",
        on_divergence: str = "fork",
    ):
        self.alias = alias
        self.tenant = tenant
        self.crypto_core = RaqimCryptoCore(private_key_path, cert_path)
        self.cert_bytes = bytes(self.crypto_core.capability_cert_bytes)
        self.cert_hex = self.cert_bytes.hex()
        self.workload_token = workload_token

        public_key_bytes = bytes(self.crypto_core.pub_key_bytes)
        derived_16_bytes = blake3.blake3(
            public_key_bytes, derive_key_context="raqim.agent.v1.identity"
        ).digest(length=16)

        self.agent_hex = derived_16_bytes.hex()
        self.tcp_addr = (daemon_host, tcp_port)
        self.http_url = f"http://{daemon_host}:{http_port}"

        self.mode = mode
        self.on_divergence = on_divergence
        self.is_forked = False
        self.last_tx_id: Optional[str] = None
        self.recorded_tx_ids: Dict[int, str] = {}
        self._step_metadata: Dict[int, TraceMetadata] = {}
        self._http_clients_by_loop: Dict[Any, httpx.AsyncClient] = {}

    def _get_http_client(self) -> httpx.AsyncClient:
        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:
            loop = None

        client = self._http_clients_by_loop.get(loop)
        if client is None or client.is_closed:
            client = httpx.AsyncClient(timeout=15.0)
            self._http_clients_by_loop[loop] = client
        return client

    async def boot(self) -> None:
        """Registers the agent's alias and cryptographic identity with the local Raqim daemon."""
        await self.commit_record(
            agent_hex=self.agent_hex,
            namespace="/system/handshake",
            text=f"ALIAS={self.alias}",
        )

    async def commit_record(self, *args: Any, **kwargs: Any) -> int:
        """
        Transmits a signed zero-copy rkyv `ExecutionRecord` over raw TCP and awaits
        the synchronous 20-byte `fdatasync` group-commit ACK containing the 128-bit TxID.
        """
        agent_hex = kwargs.get("agent_hex")
        namespace = kwargs.get("namespace") or kwargs.get("intent_path")
        text = kwargs.get("text")

        if len(args) == 3:
            agent_hex, namespace, text = args
        elif len(args) == 2:
            namespace, text = args
        elif len(args) == 1:
            namespace = args[0]

        target_agent = agent_hex or self.agent_hex
        if not namespace or text is None:
            raise ValueError("Both 'namespace' and 'text' are required for commit_record.")

        raw_payload = self.crypto_core.generate_tcp_payload(target_agent, namespace, text)

        reader, writer = await asyncio.open_connection(*self.tcp_addr)
        try:
            writer.write(raw_payload)
            await writer.drain()

            ack_bytes = await reader.readexactly(20)
            status = int.from_bytes(ack_bytes[0:4], "little")
            if status != 0:
                raise PermissionError(
                    f"[POLICY REJECTION] TCP frame rejected by kernel (Status: {status})"
                )
            return int.from_bytes(ack_bytes[4:20], "little")
        finally:
            writer.close()
            await writer.wait_closed()

    # Backwards-compatible alias
    commit_thought = commit_record

    @contextlib.asynccontextmanager
    async def open_stream(self):
        """Maintains a persistent TCP connection for high-throughput batch streaming."""
        reader, writer = await asyncio.open_connection(*self.tcp_addr)

        async def send(namespace: str, text: str) -> int:
            raw_payload = self.crypto_core.generate_tcp_payload(self.agent_hex, namespace, text)
            writer.write(raw_payload)
            await writer.drain()

            ack_bytes = await reader.readexactly(20)
            status = int.from_bytes(ack_bytes[0:4], "little")
            if status != 0:
                raise PermissionError(f"[POLICY REJECTION] Ingress dropped (Status: {status})")
            return int.from_bytes(ack_bytes[4:20], "little")

        try:
            yield send
        finally:
            writer.close()
            await writer.wait_closed()

    async def egress_request(
        self,
        method: str,
        connector: str,
        subpath: str,
        json_body: Optional[Dict[str, Any]] = None,
        step_ordinal: Optional[int] = None,
        raw_body: Optional[bytes] = None,
    ) -> httpx.Response:
        """
        Dispatches a zero-secret HTTP call through `/v1/egress/{connector}/{subpath}`
        using the agent's `rqm_v1` workload token.
        Pass `raw_body` to send exact wire bytes (e.g., adversarial parser tests).
        """
        if not self.workload_token:
            raise RaqimClientError("workload_token (`rqm_v1...`) is required for egress_request")

        clean_sub = subpath.lstrip("/")
        url = f"{self.http_url}/v1/egress/{connector}/{clean_sub}"

        headers = {
            "Authorization": f"Bearer {self.workload_token}",
            "X-Raqim-Mode": self.mode,
        }
        if step_ordinal is not None:
            headers["X-Raqim-Step"] = str(step_ordinal)

        http = self._get_http_client()
        if raw_body is not None:
            headers.setdefault("Content-Type", "application/json")
            return await http.request(method.upper(), url, headers=headers, content=raw_body)
        return await http.request(method.upper(), url, headers=headers, json=json_body)
    
    def trace(
        self,
        namespace: str = "/default",
        model: Optional[str] = None,
        system: Optional[str] = None,
        custom_signature: Optional[str] = None,
    ) -> Callable[..., Any]:
        """
        @client.trace Decorator:
        Wraps any sync function, async coroutine, or async streaming generator.
        - In 'record' mode: Authorizes pre-flight Intent, runs function live, seals Outcome to WAL.
        - In 'replay' mode: Bypasses execution and returns recorded output in <1ms ($0 API cost).
        - On input/prompt change: Auto-forks execution onto an isolated `phantom_` branch.
        """

        def decorator(fn: Callable[..., Any]) -> Callable[..., Any]:
            if inspect.isasyncgenfunction(fn):

                @functools.wraps(fn)
                async def async_gen_wrapper(*args: Any, **kwargs: Any) -> AsyncGenerator[Any, None]:
                    step = _execution_step_context.get()
                    _execution_step_context.set(step + 1)

                    call_sig_hex, _ = CanonicalSerializer.derive_call_signature_hash(
                        fn, args, kwargs, custom_signature
                    )

                    target_ns = namespace
                    if self.is_forked:
                        target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"

                    if self.mode == "replay" and not self.is_forked:
                        cached = await self._fetch_recorded_effect(step, call_sig_hex)
                        if cached is not None:
                            for chunk in cached:
                                yield chunk
                            return

                        self._handle_divergence(step, call_sig_hex, namespace)
                        target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"

                    await self._authorize_intent(step, call_sig_hex, target_ns)

                    accumulated_chunks: List[Any] = []
                    async for item in fn(*args, **kwargs):
                        accumulated_chunks.append(item)
                        yield item

                    meta = TraceMetadata.from_response(accumulated_chunks, default_model=model)
                    if system:
                        meta.system = system
                    self._step_metadata[step] = meta

                    await self._persist_effect(step, call_sig_hex, accumulated_chunks, target_ns)

                return async_gen_wrapper

            elif asyncio.iscoroutinefunction(fn):

                @functools.wraps(fn)
                async def async_wrapper(*args: Any, **kwargs: Any) -> Any:
                    step = _execution_step_context.get()
                    _execution_step_context.set(step + 1)

                    call_sig_hex, _ = CanonicalSerializer.derive_call_signature_hash(
                        fn, args, kwargs, custom_signature
                    )

                    target_ns = namespace
                    if self.is_forked:
                        target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"

                    if self.mode == "replay" and not self.is_forked:
                        cached = await self._fetch_recorded_effect(step, call_sig_hex)
                        if cached is not None:
                            return cached

                        self._handle_divergence(step, call_sig_hex, namespace)
                        target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"

                    await self._authorize_intent(step, call_sig_hex, target_ns)

                    result = await fn(*args, **kwargs)

                    meta = TraceMetadata.from_response(result, default_model=model)
                    if system:
                        meta.system = system
                    self._step_metadata[step] = meta

                    await self._persist_effect(step, call_sig_hex, result, target_ns)
                    return result

                return async_wrapper

            else:

                @functools.wraps(fn)
                def sync_wrapper(*args: Any, **kwargs: Any) -> Any:
                    step = _execution_step_context.get()
                    _execution_step_context.set(step + 1)

                    call_sig_hex, _ = CanonicalSerializer.derive_call_signature_hash(
                        fn, args, kwargs, custom_signature
                    )

                    target_ns = namespace
                    if self.is_forked:
                        target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"

                    try:
                        running_loop = asyncio.get_running_loop()
                    except RuntimeError:
                        running_loop = None

                    if running_loop and running_loop.is_running():
                        if self.mode == "replay" and not self.is_forked:
                            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                                cached = pool.submit(
                                    lambda: asyncio.run(self._fetch_recorded_effect(step, call_sig_hex))
                                ).result()
                            if cached is not None:
                                return cached

                            self._handle_divergence(step, call_sig_hex, namespace)
                            target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"

                        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                            pool.submit(
                                lambda: asyncio.run(self._authorize_intent(step, call_sig_hex, target_ns))
                            ).result()

                        result = fn(*args, **kwargs)

                        meta = TraceMetadata.from_response(result, default_model=model)
                        if system:
                            meta.system = system
                        self._step_metadata[step] = meta

                        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                            pool.submit(
                                lambda: asyncio.run(
                                    self._persist_effect(step, call_sig_hex, result, target_ns)
                                )
                            ).result()
                        return result
                    else:
                        loop = self._get_or_create_event_loop()
                        if self.mode == "replay" and not self.is_forked:
                            cached = loop.run_until_complete(
                                self._fetch_recorded_effect(step, call_sig_hex)
                            )
                            if cached is not None:
                                return cached

                            self._handle_divergence(step, call_sig_hex, namespace)
                            target_ns = f"phantom_{namespace}_{self.agent_hex}_step{step}"

                        loop.run_until_complete(self._authorize_intent(step, call_sig_hex, target_ns))
                        result = fn(*args, **kwargs)

                        meta = TraceMetadata.from_response(result, default_model=model)
                        if system:
                            meta.system = system
                        self._step_metadata[step] = meta

                        loop.run_until_complete(
                            self._persist_effect(step, call_sig_hex, result, target_ns)
                        )
                        return result

                return sync_wrapper

        return decorator

    observe_agent = trace
    flight_recorder = trace

    async def _authorize_intent(
        self, step_ordinal: int, call_signature_hex: str, namespace: str
    ) -> None:
        now_ts = int(time.time())
        attestation_payload = (
            f"preflight:{namespace}:{step_ordinal}:{call_signature_hex}:{now_ts}".encode("utf-8")
        )
        signature_bytes = bytes(self.crypto_core.sign_payload(attestation_payload))
        pub_key_bytes = bytes(self.crypto_core.pub_key_bytes)

        payload = {
            "agent_hex": self.agent_hex,
            "public_key_hex": pub_key_bytes.hex(),
            "signature_hex": signature_bytes.hex(),
            "capability_cert_hex": self.cert_hex,
            "timestamp": now_ts,
            "step_ordinal": step_ordinal,
            "call_signature_hex": call_signature_hex,
            "namespace": namespace,
        }

        http = self._get_http_client()
        resp = await http.post(f"{self.http_url}/v1/effect/preflight", json=payload)
        if resp.status_code in (401, 403, 429):
            try:
                error_msg = resp.json().get("message", resp.text)
            except Exception:
                error_msg = resp.text
            raise PermissionError(f"[POLICY PRE-FLIGHT INTERDICTION]: {error_msg}")

        if resp.status_code != 200:
            raise RuntimeError(
                f"Daemon preflight check failed (HTTP {resp.status_code}): {resp.text}"
            )

    async def _persist_effect(
        self, step_ordinal: int, call_signature_hex: str, result: Any, namespace: str
    ) -> None:
        canonical_output = CanonicalSerializer.canonical_json(result)
        b64_output = base64.b64encode(canonical_output.encode("utf-8")).decode("ascii")
        now_ts = int(time.time())

        attestation_payload = (
            f"{namespace}:{step_ordinal}:{call_signature_hex}:{b64_output}:{now_ts}".encode("utf-8")
        )
        signature_bytes = bytes(self.crypto_core.sign_payload(attestation_payload))
        pub_key_bytes = bytes(self.crypto_core.pub_key_bytes)

        payload = {
            "agent_hex": self.agent_hex,
            "public_key_hex": pub_key_bytes.hex(),
            "signature_hex": signature_bytes.hex(),
            "capability_cert_hex": self.cert_hex,
            "timestamp": now_ts,
            "step_ordinal": step_ordinal,
            "call_signature_hex": call_signature_hex,
            "namespace": namespace,
            "output_payload_base64": b64_output,
        }

        http = self._get_http_client()
        resp = await http.post(f"{self.http_url}/v1/effect/record", json=payload)

        if resp.status_code in (401, 403, 429):
            try:
                error_msg = resp.json().get("message", resp.text)
            except Exception:
                error_msg = resp.text
            raise PermissionError(f"[POLICY INTERDICTION]: {error_msg}")

        if resp.status_code != 200:
            raise RuntimeError(f"Daemon rejected effect (HTTP {resp.status_code}): {resp.text}")

        resp_data = resp.json()
        tx_id = resp_data.get("tx_id_hex")
        if tx_id:
            self.last_tx_id = tx_id
            self.recorded_tx_ids[step_ordinal] = tx_id

    async def _fetch_recorded_effect(self, step_ordinal: int, call_sig_hex: str) -> Optional[Any]:
        http = self._get_http_client()
        try:
            res = await http.post(
                f"{self.http_url}/v1/effect/get",
                json={
                    "agent_hex": self.agent_hex,
                    "step_ordinal": step_ordinal,
                    "call_signature_hex": call_sig_hex,
                },
                timeout=5.0,
            )
            if res.status_code == 200:
                data = res.json()
                if data.get("found") and data.get("output_payload_base64"):
                    raw_bytes = base64.b64decode(data["output_payload_base64"])
                    return json.loads(raw_bytes.decode("utf-8"))
        except Exception as e:
            print(f"[RAQIM REPLAY WARN] Effect fetch error at step {step_ordinal}: {e}")
        return None

    def _handle_divergence(self, step: int, call_sig_hex: str, namespace: str) -> None:
        if self.on_divergence == "raise":
            raise ReplayDivergedError(
                f"[RAQIM REPLAY DIVERGED] Code/prompt modified at Step {step} "
                f"(Signature: {call_sig_hex[:8]}...). No recorded trace matches history."
            )
        self.is_forked = True

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

    async def export_to_otel(
        self,
        endpoint: str,
        headers: Optional[Dict[str, str]] = None,
        timeout: float = 10.0,
    ) -> Dict[str, Any]:
        """Exports the agent's cryptographically verified execution timeline as CNCF OTLP/HTTP JSON."""
        daemon_otel_url = f"{self.http_url}/v1/session/timeline/{self.agent_hex}/export/otel"
        async with httpx.AsyncClient(timeout=timeout) as http:
            res = await http.get(daemon_otel_url)
            if res.status_code != 200:
                raise RaqimClientError(
                    f"Daemon failed to synthesize OTLP trace (HTTP {res.status_code}): {res.text}"
                )
            otlp_payload = res.json()

        resource = otlp_payload["resourceSpans"][0]["resource"]["attributes"]
        resource.append({"key": "raqim.agent_alias", "value": {"stringValue": self.alias}})
        resource.append({"key": "raqim.execution_mode", "value": {"stringValue": self.mode}})
        resource.append({"key": "raqim.is_forked", "value": {"boolValue": self.is_forked}})

        for span in otlp_payload["resourceSpans"][0]["scopeSpans"][0]["spans"]:
            for attr in span["attributes"]:
                if attr["key"] == "raqim.step_ordinal":
                    step = attr["value"].get("intValue")
                    if step in self._step_metadata:
                        step_meta = self._step_metadata[step]
                        if step_meta.model:
                            span["attributes"].append(
                                {
                                    "key": "gen_ai.request.model",
                                    "value": {"stringValue": step_meta.model},
                                }
                            )
                        if step_meta.prompt_tokens is not None:
                            span["attributes"].append(
                                {
                                    "key": "gen_ai.usage.input_tokens",
                                    "value": {"intValue": step_meta.prompt_tokens},
                                }
                            )
                        if step_meta.completion_tokens is not None:
                            span["attributes"].append(
                                {
                                    "key": "gen_ai.usage.output_tokens",
                                    "value": {"intValue": step_meta.completion_tokens},
                                }
                            )

        req_headers = {"Content-Type": "application/json"}
        if headers:
            req_headers.update(headers)

        async with httpx.AsyncClient(timeout=timeout) as http:
            post_resp = await http.post(endpoint, json=otlp_payload, headers=req_headers)
            success = post_resp.status_code in (200, 202)
            span_count = len(otlp_payload["resourceSpans"][0]["scopeSpans"][0]["spans"])
            return {
                "success": success,
                "status_code": post_resp.status_code,
                "spans_exported": span_count,
                "target_endpoint": endpoint,
                "agent_hex": self.agent_hex,
            }


def verify_state_proof_offline(
    payload_bytes: Optional[bytes],
    agent_id_str: str,
    proof_dict: Dict[str, Any],
) -> bool:
    """
    Zero-Dependency Offline Cryptographic Merkle Proof Verifier (v0.2.0).
    - If `payload_bytes` is provided (unredacted record): verifies BOTH the salted
      payload commitment (`raqim.payload.v1`) AND the full 9-field envelope leaf hash (`raqim.merkle.v1.leaf`).
    - If `payload_bytes` is None (NDPA/GDPR redacted record): verifies the full
      envelope leaf hash directly from `payloadHashHex` in `proof_dict`!
    """
    try:
        tx_id_hex = (proof_dict.get("txIdHex") or proof_dict.get("tx_id_hex", "")).removeprefix(
            "0x"
        )
        tx_id = int(tx_id_hex, 16)
        agent_id_bytes = bytes.fromhex(agent_id_str)
        step_ordinal = int(proof_dict.get("stepOrdinal", proof_dict.get("step_ordinal", 0)))
        timestamp_ms = int(proof_dict.get("timestampMs", proof_dict.get("timestamp_ms", 0)))
        stage = int(proof_dict.get("stage", 2))
        namespace = proof_dict.get("namespace", "")
        call_sig_hex = proof_dict.get("callSignatureHex") or proof_dict.get(
            "call_signature_hex", ""
        )
        salt_hex = proof_dict.get("payloadSaltHex") or proof_dict.get("payload_salt_hex", "")
        expected_payload_hash_hex = proof_dict.get("payloadHashHex") or proof_dict.get(
            "payload_hash_hex", ""
        )

        call_sig_bytes = bytes.fromhex(call_sig_hex)
        salt_bytes = bytes.fromhex(salt_hex)
        expected_payload_hash = bytes.fromhex(expected_payload_hash_hex)
    except Exception:
        return False

    # 1. If unredacted payload_bytes is provided, verify salted commitment
    if payload_bytes is not None:
        p_hasher = blake3.blake3(derive_key_context="raqim.payload.v1")
        p_hasher.update(salt_bytes)
        p_hasher.update(payload_bytes)
        computed_payload_hash = p_hasher.digest(length=32)
        if computed_payload_hash != expected_payload_hash:
            return False
    else:
        computed_payload_hash = expected_payload_hash

    # 2. Recompute full v0.2.0 action envelope leaf hash ("raqim.merkle.v1.leaf")
    ns_bytes = namespace.encode("utf-8")
    leaf_hasher = blake3.blake3(derive_key_context="raqim.merkle.v1.leaf")
    leaf_hasher.update(tx_id.to_bytes(16, "little"))
    leaf_hasher.update(agent_id_bytes)
    leaf_hasher.update(step_ordinal.to_bytes(8, "little"))
    leaf_hasher.update(timestamp_ms.to_bytes(8, "little", signed=True))
    leaf_hasher.update(bytes([stage]))
    leaf_hasher.update(len(ns_bytes).to_bytes(4, "little"))
    leaf_hasher.update(ns_bytes)
    leaf_hasher.update(call_sig_bytes)
    leaf_hasher.update(computed_payload_hash)
    current_hash = leaf_hasher.digest(length=32)

    # 3. Traverse Merkle path with odd-node domain separation ("raqim.merkle.v1.node_odd")
    index = int(proof_dict.get("leafIndex", proof_dict.get("leaf_index", 0)))
    level_len = int(proof_dict.get("totalLeavesInTree", proof_dict.get("total_leaves_in_tree", 1)))
    sibling_hashes = proof_dict.get("siblingHashesHex") or proof_dict.get("sibling_hashes_hex", [])
    merkle_root = proof_dict.get("merkleRootHex") or proof_dict.get("merkle_root_hex", "")

    for sibling_hex in sibling_hashes:
        sibling_bytes = bytes.fromhex(sibling_hex)
        if index % 2 == 0:
            if index + 1 < level_len:
                node_hasher = blake3.blake3(derive_key_context="raqim.merkle.v1.node")
                node_hasher.update(current_hash)
                node_hasher.update(sibling_bytes)
                current_hash = node_hasher.digest(length=32)
            else:
                odd_hasher = blake3.blake3(derive_key_context="raqim.merkle.v1.node_odd")
                odd_hasher.update(current_hash)
                current_hash = odd_hasher.digest(length=32)
        else:
            node_hasher = blake3.blake3(derive_key_context="raqim.merkle.v1.node")
            node_hasher.update(sibling_bytes)
            node_hasher.update(current_hash)
            current_hash = node_hasher.digest(length=32)

        index //= 2
        level_len = (level_len + 1) // 2

    return current_hash.hex() == merkle_root