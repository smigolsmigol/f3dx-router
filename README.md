# f3dx-router

[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/smigolsmigol/f3dx-router/badge)](https://scorecard.dev/viewer/?uri=github.com/smigolsmigol/f3dx-router)

LiteLLM is the incumbent Python router. It works but it's slow (~500us mean overhead per call by their own troubleshooting docs), and the Hono / Cloudflare gateways like Helicone went into maintenance mode in March 2026 after the Mintlify acquisition. The in-process Rust hot path is the gap.

`f3dx-router` is what you import inside an agent loop when the network hop to a hosted gateway is too expensive. It composes with hosted billing/dashboard products like [llmkit](https://llmkit.sh) instead of competing with them: same SDK, two backends. Local f3dx-router for the sub-millisecond hot path; llmkit for the cost dashboard. Or both.

```bash
pip install f3dx-router
```

```python
from f3dx_router import Router

r = Router(
    providers=[
        {"name": "openai", "kind": "openai",
         "base_url": "https://api.openai.com/v1",
         "api_key": "sk-..."},
        {"name": "groq", "kind": "openai",
         "base_url": "https://api.groq.com/openai/v1",
         "api_key": "gsk_..."},
    ],
    policy="hedged",
    hedge_k=2,
)

response = r.chat_completions({
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "hi"}],
})
print(response["choices"][0]["message"]["content"])
```

## Routing policies

| Policy | What it does | Cost vs latency |
|---|---|---|
| `sequential` | Fire to providers in order; on 429 / 5xx / timeout fall through to the next | Lowest cost, highest latency on failures |
| `hedged` | Fire to top-K in parallel; return first non-error, cancel the rest | Higher cost, latency = min over K |

V0.1 adds `weighted` (round-robin with per-provider weights) and the killer feature: schema-pass-rate sliding window classifier wired to f3dx-trace, routing away from a degrading provider automatically. Track validation-failure-rate per (provider, tenant, model) over the last N requests; if it crosses a threshold, drop that provider's weight to zero until the rate recovers.

## Failure model

| Status | Class | Behavior |
|---|---|---|
| 2xx | success | Return the body |
| 408 / 429 / 5xx | soft | Try the next provider in the policy |
| 4xx (other) | hard | Surface immediately - retrying elsewhere with the same payload makes things worse |
| Connection reset, timeout | soft | Same as 5xx |

The hard-failure short-circuit matters: routing a 401 (bad API key) to the next provider just leaks the same bad key to a second vendor.

## Architecture

```
f3dx-router/
  crates/
    f3dx-router/      core: Provider, RouterConfig, Router (Rust)
    f3dx-router-py/   PyO3 bridge cdylib (the only crate with #[pymodule])
  python/
    f3dx_router/__init__.py  Router class wrapping the native PyO3 surface
```

Connection pooling via reqwest's per-host pool (16 idle conns / host). Tokio runtime constructed once at Router instantiation; every `chat_completions` call reuses it under `py.allow_threads` so concurrent callers don't deadlock on the GIL.

## Composes with llmkit

[llmkit](https://github.com/smigolsmigol/llmkit) at llmkit.sh is the hosted gateway: TypeScript on Cloudflare Workers, OpenAI / Anthropic / Gemini provider abstraction, budget enforcement, cost dashboards, npm + PyPI SDKs. The clean separation:

- **llmkit**: hosted, multi-tenant, billing, dashboards, audit logs. Network hop required.
- **f3dx-router**: in-process, single-tenant, sub-ms swap. No network hop.

You can use both: f3dx-router as the local hot path with llmkit configured as one of its `providers` for the cost-tracking sink. Best of both.

## What this is not

`f3dx-router` is not a hosted gateway. Use [llmkit](https://llmkit.sh), [Helicone](https://helicone.ai), [Portkey](https://portkey.ai), or [OpenRouter](https://openrouter.ai) for that.

`f3dx-router` is not LiteLLM. LiteLLM has 100+ provider adapters; this V0 has two (OpenAI-shape, Anthropic-shape). The differentiator is in-process latency, not provider coverage.

`f3dx-router` is not a multi-region failover system. Region awareness lives in your provider config; the router just picks among configured providers.

## Sibling projects

The f3d1 ecosystem:

- [`f3dx`](https://github.com/smigolsmigol/f3dx) - Rust runtime your Python imports. Drop-in for openai + anthropic SDKs with native SSE streaming, agent loop with concurrent tool dispatch, OTel emission. `pip install f3dx`.
- [`tracewright`](https://github.com/smigolsmigol/tracewright) - Trace-replay adapter for `pydantic-evals`. Read an f3dx or pydantic-ai logfire JSONL trace, get a `pydantic_evals.Dataset`. `pip install tracewright`.
- [`f3dx-cache`](https://github.com/smigolsmigol/f3dx-cache) - Content-addressable LLM response cache + replay. redb + RFC 8785 JCS + BLAKE3. `pip install f3dx-cache`.
- [`pydantic-cal`](https://github.com/smigolsmigol/pydantic-cal) - Calibration metrics for `pydantic-evals`: ECE, MCE, ACE, Brier, reliability diagrams, Fisher-Rao geometry kernel. `pip install pydantic-cal`.
- [`f3dx-bench`](https://github.com/smigolsmigol/f3dx-bench) - Public real-prod-traffic LLM benchmark dashboard. CF Worker + R2 + duckdb-wasm. [Live](https://f3dx-bench.pages.dev).
- [`llmkit`](https://github.com/smigolsmigol/llmkit) - Hosted API gateway with budget enforcement, session tracking, cost dashboards, MCP server. [llmkit.sh](https://llmkit.sh).
- [`keyguard`](https://github.com/smigolsmigol/keyguard) - Security linter for open source projects. Finds and fixes what others only report.

## Roadmap

| Version | What |
|---|---|
| v0.0.1 | Sequential + hedged policies, OpenAI + Anthropic provider kinds, 429/5xx soft-failure routing |
| v0.0.2 | Weighted round-robin, per-provider rate-limit budgets via token-bucket |
| v0.1.0 | Schema-pass-rate sliding window classifier (the killer feature) wired to f3dx-trace |
| v0.2.0 | RouteLLM weights wrapped behind `f3dx-router[smart]` extra |
| v0.3.0 | Wire the same crate to wasm32-unknown-unknown + drop into llmkit's CF Worker as the new hot path |

## License

MIT.
