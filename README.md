# f3dx-router (archived)

This repository is frozen. The maintained router implementation moved to
[`f3dx`](https://github.com/smigolsmigol/f3dx) in April 2026.

New code should install and import the maintained package:

```bash
pip install "f3dx[router]"
```

```python
from f3dx.router import Router
```

## Migration

| Deprecated | Maintained |
|---|---|
| `pip install f3dx-router` | `pip install "f3dx[router]"` |
| `from f3dx_router import Router` | `from f3dx.router import Router` |

`f3dx-router==0.0.4` remains a compatibility shim on PyPI. It depends on the `f3dx` 0.0.x line,
emits `DeprecationWarning`, and re-exports `f3dx.router.Router`. Existing deployments can pin that
version while they migrate. Verify current package availability on
[PyPI](https://pypi.org/project/f3dx-router/) rather than relying on a dated removal schedule.

## Support

Open new runtime issues and changes in
[`smigolsmigol/f3dx`](https://github.com/smigolsmigol/f3dx). This repository is retained for source
history and migration context only.

MIT licensed.
