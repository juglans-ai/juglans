"""
Juglans Python SDK — run .jg workflows from Python.

Requires the `juglans` CLI to be installed and available on PATH.

Usage:
    import juglans

    # One-shot (returns parsed JSON output)
    output = juglans.run("main.jg", input={"query": "hello"})

    # Streaming (yields SSE events as dicts)
    for event in juglans.stream("main.jg", input={"query": "hello"}):
        if event["event"] == "token":
            print(event["content"], end="", flush=True)

    # Builder pattern
    output = (juglans.Runner("main.jg")
        .input({"query": "hello"})
        .env({"OPENAI_API_KEY": "sk-..."})
        .timeout(60)
        .run())
"""

import json
import subprocess
import os
from typing import Any, Dict, Iterator, Optional


class JuglansError(Exception):
    """Raised when the juglans CLI exits with a non-zero code."""

    def __init__(self, stderr: str, returncode: int = 1):
        self.stderr = stderr
        self.returncode = returncode
        super().__init__(stderr.strip() or f"juglans exited with code {returncode}")


def run(
    file_path: str,
    *,
    input: Optional[Dict[str, Any]] = None,
    cwd: Optional[str] = None,
    timeout: Optional[int] = 300,
    env: Optional[Dict[str, str]] = None,
) -> Any:
    """Run a .jg workflow and return the output as parsed JSON."""
    cmd = ["juglans", file_path, "--output-format", "json"]
    if input is not None:
        cmd += ["--input", json.dumps(input)]

    run_env = None
    if env:
        run_env = {**os.environ, **env}

    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        cwd=cwd,
        timeout=timeout,
        env=run_env,
    )

    if result.returncode != 0:
        raise JuglansError(result.stderr, result.returncode)

    stdout = result.stdout.strip()
    if not stdout:
        return None
    return json.loads(stdout)


def stream(
    file_path: str,
    *,
    input: Optional[Dict[str, Any]] = None,
    cwd: Optional[str] = None,
    env: Optional[Dict[str, str]] = None,
) -> Iterator[Dict[str, Any]]:
    """Run a .jg workflow in SSE mode, yielding events as dicts."""
    cmd = ["juglans", file_path, "--output-format", "sse"]
    if input is not None:
        cmd += ["--input", json.dumps(input)]

    run_env = None
    if env:
        run_env = {**os.environ, **env}

    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        cwd=cwd,
        env=run_env,
    )

    try:
        for line in proc.stdout:
            line = line.strip()
            if line.startswith("data: "):
                payload = line[6:]
                try:
                    yield json.loads(payload)
                except json.JSONDecodeError:
                    yield {"event": "raw", "data": payload}
    finally:
        proc.wait()
        if proc.returncode != 0:
            stderr = proc.stderr.read() if proc.stderr else ""
            raise JuglansError(stderr, proc.returncode)


class Runner:
    """Builder for configuring and running a Juglans workflow."""

    def __init__(self, file_path: str):
        self._file_path = file_path
        self._input: Optional[Dict[str, Any]] = None
        self._cwd: Optional[str] = None
        self._timeout: Optional[int] = 300
        self._env: Optional[Dict[str, str]] = None

    def input(self, data: Dict[str, Any]) -> "Runner":
        self._input = data
        return self

    def cwd(self, path: str) -> "Runner":
        self._cwd = path
        return self

    def timeout(self, seconds: int) -> "Runner":
        self._timeout = seconds
        return self

    def env(self, env_vars: Dict[str, str]) -> "Runner":
        self._env = env_vars
        return self

    def run(self) -> Any:
        return run(
            self._file_path,
            input=self._input,
            cwd=self._cwd,
            timeout=self._timeout,
            env=self._env,
        )

    def stream(self) -> Iterator[Dict[str, Any]]:
        return stream(
            self._file_path,
            input=self._input,
            cwd=self._cwd,
            env=self._env,
        )
