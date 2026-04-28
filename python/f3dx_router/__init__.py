"""f3dx-router: in-process Rust router for LLM providers.

Composes with hosted gateways like llmkit. The hosted gateway owns
billing, dashboards, multi-tenant config. f3dx-router owns the
in-process Rust hot path inside an agent loop where the network hop
to a hosted gateway is too expensive.

Two policies in V0:
  sequential   try providers in order; fall through on 429/5xx
  hedged       fire to top-K in parallel; return first non-error

V0.1 adds weighted round-robin + Pydantic-validation-driven routing
(track schema-pass-rate per (provider, tenant) sliding window, route
away from a degrading provider) wired to f3dx-trace.
"""
from __future__ import annotations

import json
from collections.abc import Mapping
from typing import Any

from f3dx_router._native import Router as _NativeRouter

__all__ = ["Router"]


class Router:
    """LLM provider router.

    >>> r = Router(
    ...     providers=[
    ...         {"name": "openai", "kind": "openai",
    ...          "base_url": "https://api.openai.com/v1",
    ...          "api_key": "sk-..."},
    ...         {"name": "groq", "kind": "openai",
    ...          "base_url": "https://api.groq.com/openai/v1",
    ...          "api_key": "gsk_..."},
    ...     ],
    ...     policy="hedged",
    ...     hedge_k=2,
    ... )
    >>> response = r.chat_completions({
    ...     "model": "gpt-4o",
    ...     "messages": [{"role": "user", "content": "hi"}],
    ... })
    """

    def __init__(
        self,
        providers: list[Mapping[str, Any]],
        *,
        policy: str = "sequential",
        hedge_k: int = 2,
    ) -> None:
        self._inner = _NativeRouter(list(providers), policy, hedge_k)

    def chat_completions(self, body: Mapping[str, Any]) -> dict[str, Any]:
        return self._inner.chat_completions(json.dumps(body))
