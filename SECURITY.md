# Security Policy

## Reporting a vulnerability

Email smigolsmigol@protonmail.com. Do not open a public issue for anything you suspect is exploitable.

Acknowledgment within 48 hours. Fix or mitigation for critical issues within 7 days; lower-severity issues are scheduled into the next release.

If you want PGP, ping the email above and a key will be returned.

## Supported versions

| Version | Supported |
|---|---|
| latest | Yes |
| older  | No  |

Only the most recent published wheel on PyPI is patched. Pin a version in your lockfile and upgrade on advisory.

## Architecture surface (what you are trusting)

- Rust core crate (`f3dx-router`) plus a PyO3 bridge crate (`f3dx-router-py`). The bridge is the only crate exposing a `#[pymodule]`.
- Outbound HTTPS via `reqwest` with `rustls` (no native OpenSSL, no system trust store dependency).
- In-process router. No daemon, no listening port, no IPC. Failure blast radius is the calling Python process.
- API keys are passed in by the caller and held in the `Router` instance for the process lifetime. Scrub them out of logs at the application layer; the router does not log payloads.

## Out of scope

- Misconfiguration in your provider list (wrong base_url, leaked key in source).
- Attacks against the upstream model providers themselves.
- Issues in `maturin` or `PyO3` toolchains; report those upstream.
