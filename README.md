# f3dx-router (DEPRECATED)

> **This package has moved.** As of 2026-04-30, `f3dx-router` is consolidated into `f3dx` as a Python sub-module + Cargo workspace member. Install the new home and update your imports.

```bash
pip install f3dx[router]
```

```python
from f3dx.router import Router

router = Router(
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
```

## Why moved

Single wheel, single ABI, single CI, single release cadence. The router is one of f3dx's runtime layers (alongside the agent runtime, HTTP clients, cache, MCP, trace sink); shipping it as a separate package created cross-repo version drift and discoverability friction. See the consolidation reasoning at [smigolsmigol/f3dx](https://github.com/smigolsmigol/f3dx).

## Transition timeline

- **v0.0.4** (this version, 2026-04-30): re-exports from `f3dx.router`, emits `DeprecationWarning` on import. Install pulls in `f3dx>=0.0.18` automatically.
- **+4 weeks** (2026-05-28): this GitHub repo flips to read-only / archived. PyPI installs of `f3dx-router==0.0.4` continue to work.
- **+4-6 months** (2026-08 to 2026-10): all `f3dx-router` versions on PyPI get yanked. Cached wheels still resolve for old installs; new `pip install f3dx-router` will fail by then.

## Migration

| Old | New |
|---|---|
| `pip install f3dx-router` | `pip install f3dx[router]` |
| `from f3dx_router import Router` | `from f3dx.router import Router` |

The Router API is identical: same constructor, same `chat_completions()` method, same sequential / hedged policies, same provider dict shape.

## License

MIT, same as the upstream f3dx project.
