"""End-to-end smoke for f3dx-router.

Spins up two stdlib HTTP mock servers that mimic OpenAI-shaped endpoints.
Tests sequential failover (first server returns 503, second returns 200)
and hedged-parallel (both servers fast, first response wins).
"""
from __future__ import annotations

import json
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from f3dx_router import Router


def make_handler(name: str, status: int, latency_ms: int = 0):
    class H(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            length = int(self.headers.get("content-length", "0"))
            _ = self.rfile.read(length)
            if latency_ms > 0:
                time.sleep(latency_ms / 1000)
            self.send_response(status)
            self.send_header("content-type", "application/json")
            body = json.dumps(
                {
                    "id": f"chatcmpl-{name}",
                    "choices": [
                        {"message": {"role": "assistant", "content": f"hello from {name}"}}
                    ],
                }
            ).encode()
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    return H


class _ReuseServer(ThreadingHTTPServer):
    allow_reuse_address = True


def start(port: int, handler) -> ThreadingHTTPServer:
    server = _ReuseServer(("127.0.0.1", port), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def main() -> None:
    print("== sequential failover (first 503, second 200) ==")
    s_bad = start(9091, make_handler("bad", 503))
    s_good = start(9092, make_handler("good", 200))
    try:
        r = Router(
            providers=[
                {"name": "bad", "kind": "openai", "base_url": "http://127.0.0.1:9091/v1", "api_key": "x"},
                {"name": "good", "kind": "openai", "base_url": "http://127.0.0.1:9092/v1", "api_key": "x"},
            ],
            policy="sequential",
        )
        resp = r.chat_completions({"model": "test", "messages": [{"role": "user", "content": "hi"}]})
        assert resp["choices"][0]["message"]["content"] == "hello from good", resp
        print(f"  routed past 503 to good: {resp['id']}")
        print("  -> sequential failover OK")
    finally:
        s_bad.shutdown()
        s_bad.server_close()
        s_good.shutdown()
        s_good.server_close()

    print("\n== hedged-parallel (slow vs fast, fast wins) ==")
    s_slow = start(9093, make_handler("slow", 200, latency_ms=200))
    s_fast = start(9094, make_handler("fast", 200, latency_ms=10))
    try:
        r = Router(
            providers=[
                {"name": "slow", "kind": "openai", "base_url": "http://127.0.0.1:9093/v1", "api_key": "x"},
                {"name": "fast", "kind": "openai", "base_url": "http://127.0.0.1:9094/v1", "api_key": "x"},
            ],
            policy="hedged",
            hedge_k=2,
        )
        t0 = time.perf_counter_ns()
        resp = r.chat_completions({"model": "test", "messages": [{"role": "user", "content": "hi"}]})
        elapsed_ms = (time.perf_counter_ns() - t0) / 1_000_000
        assert resp["choices"][0]["message"]["content"] == "hello from fast", resp
        assert elapsed_ms < 150, f"hedged should beat slow (~200ms), got {elapsed_ms:.1f}ms"
        print(f"  fast won: {resp['id']} in {elapsed_ms:.1f}ms (slow-only would be ~200ms)")
        print("  -> hedged-parallel OK")
    finally:
        s_slow.shutdown()
        s_slow.server_close()
        s_fast.shutdown()
        s_fast.server_close()

    print("\n== hard failure surfaces immediately (401 unauthorized) ==")
    s_auth = start(9095, make_handler("authfail", 401))
    s_good = start(9096, make_handler("good", 200))
    try:
        r = Router(
            providers=[
                {"name": "auth", "kind": "openai", "base_url": "http://127.0.0.1:9095/v1", "api_key": "wrong"},
                {"name": "good", "kind": "openai", "base_url": "http://127.0.0.1:9096/v1", "api_key": "x"},
            ],
            policy="sequential",
        )
        try:
            r.chat_completions({"model": "test", "messages": [{"role": "user", "content": "hi"}]})
            raise AssertionError("expected RuntimeError on hard auth failure")
        except RuntimeError as e:
            assert "401" in str(e), f"expected 401 in error: {e}"
            print(f"  hard 401 surfaced as expected: {str(e)[:80]}...")
            print("  -> hard-failure short-circuit OK")
    finally:
        s_auth.shutdown()
        s_auth.server_close()
        s_good.shutdown()
        s_good.server_close()

    print("\nALL SMOKE TESTS PASSED")


if __name__ == "__main__":
    main()
