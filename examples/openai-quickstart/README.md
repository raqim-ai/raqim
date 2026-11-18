# OpenAI Base-URL Swap Quickstart (`examples/openai-quickstart`)

Put Raqim in front of any OpenAI-compatible client (`openai`, `curl`, LangChain, Node/TypeScript, Go) **without changing application logic** and **without exposing your real `OPENAI_API_KEY` to your agent process**.

---

## Option A: Run the Self-Contained Verification (Zero OpenAI Key Needed)

Uses an isolated temporary directory, mock OpenAI upstream, `raqim-core`, and `raqim-cli`:

```bash
cargo build --release --bin raqim-core --bin raqim-cli
python3 examples/openai-quickstart/quickstart.py --self-test
```

---

## Option B: Live 3-Terminal Walkthrough With Your Real `OPENAI_API_KEY`

### 1. Start the Gateway With Your Real Key (Terminal 1)

```bash
cd examples/openai-quickstart
OPENAI_API_KEY="sk-your-real-openai-key" ../../target/release/raqim-core
```

### 2. Forge a Revocable Workload Token + Inspect CLI (Terminal 2)

```bash
cd examples/openai-quickstart
../../target/release/raqim-cli keys forge --name dev_bot --group openai_agent
../../target/release/raqim-cli doctor
```

### 3. Swap `OPENAI_BASE_URL` & `OPENAI_API_KEY` in Your App (Terminal 3)

Your application **never holds `sk-...`**. It holds only `./ca-keys/dev_bot.token` (`rqm_v1...`):

```bash
export OPENAI_BASE_URL="http://127.0.0.1:8081/v1/egress/openai/v1"
export OPENAI_API_KEY="$(cat ca-keys/dev_bot.token)"
```

Works with stock `curl`:

```bash
curl -sS "$OPENAI_BASE_URL/chat/completions" \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","max_tokens":150,"messages":[{"role":"user","content":"Hello"}]}'
```

Or with the stock `openai` Python SDK:

```python
import os
from openai import OpenAI

client = OpenAI(
    base_url=os.environ["OPENAI_BASE_URL"],
    api_key=os.environ["OPENAI_API_KEY"],  # rqm_v1 token, NOT sk-...
)
resp = client.chat.completions.create(
    model="gpt-4o-mini",
    max_tokens=150,
    messages=[{"role": "user", "content": "Hello"}],
)
```

---

## Policy Notes (Important)

[`policy.toml`](policy.toml) enforces JSON-Pointer limits on the **outbound request JSON** (`/max_tokens <= 1000`, rolling 1-hour requested cap `max_hourly_sum = 10000`, `required = true`). Setting `required = true` prevents an agent from omitting `max_tokens` to bypass limits; Raqim caps requested `max_tokens` cleanly rather than fetching upstream USD account billing.

To inspect or clear a quarantined agent:

```bash
../../target/release/raqim-cli policy list
../../target/release/raqim-cli policy lift <AGENT_HEX>
```
