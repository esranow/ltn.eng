#!/usr/bin/env python3
"""End-to-end test of `ltn mcp` over stdio against the real local Docker daemon.

Usage: python tests/e2e/mcp_e2e.py [path/to/ltn.exe]   (default target/debug/ltn.exe)

Builds nothing itself: build first with `cargo build -p ltn-cli` (Z3 env set).
Set LTN_E2E_TIMEOUT_SCALE (e.g. 3) on a loaded machine to stretch every wait.
The Z3 DLL must be on PATH for ltn.exe to start. Uses host ports 18300-18399.
Prints one PASS/FAIL line per step and exits nonzero on any failure.
Standard library only.
"""

import json
import os
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
APPS = os.path.join(HERE, "apps")


# Multiplier for every wait in this script. On a loaded machine (Docker
# Desktop starved of memory) replies can take minutes; raise this instead of
# editing timeouts. A timeout message carries elapsed time and free RAM so a
# load failure can be told apart from a bug.
SCALE = float(os.environ.get("LTN_E2E_TIMEOUT_SCALE", "1") or "1")


def scaled(seconds):
    return seconds * SCALE


def free_ram_mb():
    """Free physical memory in MB, or None if it cannot be read."""
    try:
        if os.name == "nt":
            import ctypes

            class MemStatus(ctypes.Structure):
                _fields_ = [
                    ("dwLength", ctypes.c_ulong), ("dwMemoryLoad", ctypes.c_ulong),
                    ("ullTotalPhys", ctypes.c_ulonglong), ("ullAvailPhys", ctypes.c_ulonglong),
                    ("ullTotalPageFile", ctypes.c_ulonglong), ("ullAvailPageFile", ctypes.c_ulonglong),
                    ("ullTotalVirtual", ctypes.c_ulonglong), ("ullAvailVirtual", ctypes.c_ulonglong),
                    ("ullAvailExtendedVirtual", ctypes.c_ulonglong),
                ]

            m = MemStatus()
            m.dwLength = ctypes.sizeof(MemStatus)
            if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(m)):
                return m.ullAvailPhys // (1024 * 1024)
            return None
        with open("/proc/meminfo") as f:
            for line in f:
                if line.startswith("MemAvailable:"):
                    return int(line.split()[1]) // 1024
    except Exception:  # noqa: BLE001
        return None
    return None


def load_note(started):
    ram = free_ram_mb()
    ram_s = f"{ram} MB" if ram is not None else "unknown"
    return f"elapsed {time.time() - started:.1f}s, free RAM {ram_s}, timeout scale x{SCALE:g}"

EXPECTED_TOOLS = {
    # ltn-engine
    "ltn_analyze", "ltn_plan", "ltn_apply", "ltn_rollback", "ltn_destroy",
    "ltn_targets", "ltn_target_add", "ltn_target_remove",
    # ltn-observe
    "ltn_status", "ltn_logs", "ltn_diagnose", "ltn_repair", "ltn_audit",
    # ltn-cloud
    "ltn_cloud_auth", "ltn_cloud_catalog", "ltn_cloud_targets",
    # ltn-mcp
    "ltn_doctor", "map_mtl_topology",
}

RESULTS = []


class StepFailed(Exception):
    pass


def report(name, ok, detail=""):
    RESULTS.append((name, ok))
    line = f"{'PASS' if ok else 'FAIL'} {name}"
    if detail:
        line += f" -- {detail}"
    print(line, flush=True)


def step(name):
    """Decorator-free helper: run fn, record PASS/FAIL, return its value or None."""
    def run(fn):
        try:
            detail = fn()
            report(name, True, detail if isinstance(detail, str) else "")
            return True
        except StepFailed as e:
            report(name, False, str(e))
        except Exception as e:  # noqa: BLE001 - any error is a failed step
            report(name, False, f"{type(e).__name__}: {e}")
        return False
    return run


def require(cond, msg):
    if not cond:
        raise StepFailed(msg)


class McpServer:
    def __init__(self, exe, env, stderr_path):
        self.stderr_file = open(stderr_path, "w", encoding="utf-8", errors="replace")
        self.stderr_path = stderr_path
        self.proc = subprocess.Popen(
            [exe, "mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.stderr_file,
            env=env,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self.lines = queue.Queue()
        self.stdout_noise = []
        self.next_id = 1
        threading.Thread(target=self._pump, daemon=True).start()

    def _pump(self):
        for line in self.proc.stdout:
            self.lines.put(line)
        self.lines.put(None)

    def notify(self, method, params=None):
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()

    def request(self, method, params=None, timeout=60):
        rid = self.next_id
        self.next_id += 1
        msg = {"jsonrpc": "2.0", "id": rid, "method": method}
        if params is not None:
            msg["params"] = params
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()
        timeout = scaled(timeout)
        started = time.time()
        deadline = started + timeout
        while True:
            left = deadline - time.time()
            if left <= 0:
                raise StepFailed(f"no reply to {method} within {timeout:.0f}s ({load_note(started)}); stderr tail: {self.stderr_tail()}")
            try:
                line = self.lines.get(timeout=left)
            except queue.Empty:
                continue
            if line is None:
                raise StepFailed(f"server exited (code {self.proc.poll()}) during {method}; stderr tail: {self.stderr_tail()}")
            try:
                reply = json.loads(line)
            except json.JSONDecodeError:
                self.stdout_noise.append(line)
                raise StepFailed(f"non-JSON line on stdout: {line[:200]!r}")
            if reply.get("id") == rid:
                return reply

    def call(self, tool, args=None, timeout=120):
        """Returns (is_error, text, parsed_json_or_None)."""
        reply = self.request("tools/call", {"name": tool, "arguments": args or {}}, timeout=timeout)
        if "error" in reply:
            raise StepFailed(f"{tool}: JSON-RPC error {reply['error']}")
        result = reply["result"]
        text = "".join(c.get("text", "") for c in result.get("content", []) if c.get("type") == "text")
        parsed = None
        if not result.get("isError"):
            try:
                parsed = json.loads(text)
            except json.JSONDecodeError:
                pass
        return bool(result.get("isError")), text, parsed

    def call_ok(self, tool, args=None, timeout=120):
        is_err, text, parsed = self.call(tool, args, timeout)
        require(not is_err, f"{tool} returned isError: {text[:800]}")
        require(isinstance(parsed, dict), f"{tool} result is not a JSON object: {text[:300]}")
        return parsed

    def stderr_tail(self, n=15):
        try:
            self.stderr_file.flush()
            with open(self.stderr_path, encoding="utf-8", errors="replace") as f:
                return " | ".join(f.read().splitlines()[-n:])
        except OSError:
            return "<unreadable>"

    def close(self):
        try:
            self.proc.stdin.close()
        except OSError:
            pass
        try:
            self.proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(timeout=10)
        self.stderr_file.close()


def docker(*args, check=False, timeout=60):
    started = time.time()
    try:
        r = subprocess.run(["docker", *args], capture_output=True, text=True, timeout=scaled(timeout))
    except subprocess.TimeoutExpired:
        raise StepFailed(f"docker {' '.join(args)} timed out ({load_note(started)})")
    if check and r.returncode != 0:
        raise StepFailed(f"docker {' '.join(args)} failed: {r.stderr.strip()[:300]}")
    return r


def http_get(url, timeout=5):
    with urllib.request.urlopen(url, timeout=timeout) as resp:
        return resp.status, resp.read().decode("utf-8", errors="replace").strip()


def wait_for_page(url, expected, seconds=20):
    seconds = scaled(seconds)
    started = time.time()
    deadline = started + seconds
    last = None
    while time.time() < deadline:
        try:
            status, body = http_get(url)
            last = f"{status} {body!r}"
            if status == 200 and body == expected:
                return body
        except Exception as e:  # noqa: BLE001
            last = f"{type(e).__name__}: {e}"
        time.sleep(0.5)
    raise StepFailed(f"GET {url} did not return {expected!r} within {seconds:.0f}s (last: {last}; {load_note(started)})")


def copy_app(src_name, dest_root, app, host_port):
    dest = os.path.join(dest_root, f"{src_name}-{app}")
    shutil.copytree(os.path.join(APPS, src_name), dest)
    toml = os.path.join(dest, "ltn.toml")
    with open(toml, encoding="utf-8") as f:
        text = f.read()
    text = re.sub(r'(?m)^name\s*=.*$', f'name = "{app}"', text)
    text = re.sub(r'(?m)^host_port\s*=.*$', f"host_port = {host_port}", text)
    with open(toml, "w", encoding="utf-8") as f:
        f.write(text)
    return dest


def app_status(srv, app):
    st = srv.call_ok("ltn_status", {"app": app})
    apps = [a for a in st.get("apps", []) if a.get("app") == app]
    require(apps, f"ltn_status has no entry for {app}: {json.dumps(st)[:400]}")
    return apps[0]


def main():
    exe = sys.argv[1] if len(sys.argv) > 1 else os.path.join(REPO, "target", "debug", "ltn.exe")
    exe = os.path.abspath(exe)
    if not os.path.isfile(exe):
        print(f"FAIL setup -- {exe} not found; build it with `cargo build -p ltn-cli`")
        return 2

    suffix = uuid.uuid4().hex[:8]
    app = f"e2e-web-{suffix}"
    app_nodocker = f"e2e-nodocker-{suffix}"
    work = tempfile.mkdtemp(prefix="ltn-e2e-")
    home = os.path.join(work, "home")
    os.makedirs(home)
    v1 = copy_app("web-v1", work, app, 18301)
    v2 = copy_app("web-v2", work, app, 18301)
    crash = copy_app("web-crash", work, app, 18301)
    v1_nodocker = copy_app("web-v1", work, app_nodocker, 18302)

    env = dict(os.environ)
    env.update({"LTN_HOME": home, "LTN_WATCH_INTERVAL_S": "2"})
    env.pop("LTN_WATCH", None)
    env.pop("LTN_DOCKER_BIN", None)
    print(f"info: exe={exe} app={app} work={work}", flush=True)

    srv = None
    srv2 = None
    state = {"endpoint": None, "plan_id": None}
    try:
        srv = McpServer(exe, env, os.path.join(work, "server1.stderr.txt"))

        def s_initialize():
            r = srv.request("initialize", {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "ltn-e2e", "version": "0"},
            })
            require("result" in r, f"initialize failed: {r}")
            require(r["result"].get("protocolVersion") == "2025-06-18", f"protocolVersion {r['result'].get('protocolVersion')}")
            require(r["result"].get("serverInfo", {}).get("name") == "ltn", "serverInfo.name != ltn")
            srv.notify("notifications/initialized")
            pong = srv.request("ping")
            require(pong.get("result") == {}, f"ping after initialized: {pong}")
        step("initialize")(s_initialize)

        def s_tools_list():
            r = srv.request("tools/list")
            names = {t["name"] for t in r["result"]["tools"]}
            for t in r["result"]["tools"]:
                require(t.get("description") and isinstance(t.get("inputSchema"), dict), f"tool {t.get('name')} lacks description/inputSchema")
            missing = sorted(EXPECTED_TOOLS - names)
            extra = sorted(names - EXPECTED_TOOLS)
            require(not missing and not extra, f"missing={missing} extra={extra}")
            return f"{len(names)} tools"
        step("tools/list matches ownership table")(s_tools_list)

        def s_doctor():
            d = srv.call_ok("ltn_doctor")
            bad = [c for c in d.get("checks", []) if not c.get("ok")]
            require(d.get("ok") is True, f"doctor not ok: {bad}")
            return "failing optional checks: " + (", ".join(c["name"] for c in bad) or "none")
        step("ltn_doctor ok")(s_doctor)

        def s_analyze():
            a = srv.call_ok("ltn_analyze", {"path": v1})
            require(a.get("name") == app, f"analyze name {a.get('name')!r} != {app!r}")
            require(a.get("port") == 8080 and a.get("host_port") == 18301, f"ports {a.get('port')}/{a.get('host_port')}")
            require(a.get("cpu_millis") == 250 and a.get("ram_mb") == 64, f"cpu/ram {a.get('cpu_millis')}/{a.get('ram_mb')}")
        step("ltn_analyze")(s_analyze)

        def s_plan():
            p = srv.call_ok("ltn_plan", {"path": v1})
            for k in ("plan_id", "app", "target_id", "provision_offer", "cost_cents_per_hour", "capacity_considered", "expires_at"):
                require(k in p, f"plan result lacks {k}: {list(p)}")
            require(p["app"] == app, f"plan app {p['app']}")
            state["plan_id"] = p["plan_id"]
            return f"target {p['target_id']}"
        step("ltn_plan")(s_plan)

        def apply_path(path, timeout=300):
            p = srv.call_ok("ltn_plan", {"path": path})
            return srv.call("ltn_apply", {"plan_id": p["plan_id"]}, timeout=timeout)

        def s_apply_v1():
            require(state["plan_id"], "no plan from previous step")
            is_err, text, a = srv.call("ltn_apply", {"plan_id": state["plan_id"]}, timeout=300)
            require(not is_err, f"apply v1 isError: {text[:800]}")
            for k in ("app", "release", "target_id", "container", "endpoint", "health"):
                require(k in a, f"apply result lacks {k}")
            require(a["endpoint"], "apply returned no endpoint")
            state["endpoint"] = a["endpoint"]
            return f"release {a['release']} at {a['endpoint']}"
        step("ltn_apply v1")(s_apply_v1)

        def s_get_v1():
            require(state["endpoint"], "no endpoint")
            wait_for_page(state["endpoint"], "v1")
        step("HTTP GET shows v1")(s_get_v1)

        def s_status_healthy():
            a = app_status(srv, app)
            require(a.get("verdict") == "healthy", f"verdict {a.get('verdict')}: {json.dumps(a)[:500]}")
        step("ltn_status healthy")(s_status_healthy)

        def s_logs():
            lg = srv.call_ok("ltn_logs", {"app": app})
            require(str(lg.get("logs", "")).strip(), f"empty logs: {json.dumps(lg)[:300]}")
            return f"{len(lg['logs'])} chars"
        step("ltn_logs non-empty")(s_logs)

        def s_apply_v2():
            is_err, text, a = apply_path(v2)
            require(not is_err, f"apply v2 isError: {text[:800]}")
            if a.get("endpoint"):
                state["endpoint"] = a["endpoint"]
            wait_for_page(state["endpoint"], "v2")
            return f"release {a.get('release')}"
        step("ltn_apply v2, page shows v2")(s_apply_v2)

        def s_kill_and_watch():
            a = app_status(srv, app)
            container = current_container(a, app)
            require(container, "cannot tell which container is current")
            killed_at = time.time()
            docker("kill", container, check=True)
            deadline = killed_at + scaled(30)
            healthy = False
            last = None
            while time.time() < deadline:
                time.sleep(1)
                last = app_status(srv, app)
                if last.get("verdict") == "healthy":
                    healthy = True
                    break
            require(healthy, f"not healthy within 30s of docker kill (last verdict {last and last.get('verdict')})")
            au = srv.call_ok("ltn_audit", {"app": app, "limit": 50})
            actions = [e.get("action", "") for e in au.get("entries", [])]
            watch = [x for x in actions if x.startswith("watch.") and not x.endswith("_failed")]
            require(watch, f"no watcher action in audit; actions: {actions}")
            wait_for_page(state["endpoint"], "v2", seconds=10)
            return f"healthy after {time.time() - killed_at:.1f}s; watcher actions {watch}"
        step("docker kill -> watcher repairs within 30s")(s_kill_and_watch)

        def s_crash():
            is_err, text, _ = apply_path(crash)
            require(is_err, f"crashing release was reported as success: {text[:500]}")
            require(re.search(r"exit", text, re.I) and "3" in text, f"error does not mention exit code 3: {text[:800]}")
            wait_for_page(state["endpoint"], "v2", seconds=15)
            return "rejected; v2 still served"
        step("crashing release rejected, v2 still served")(s_crash)

        # The watcher would restart a deliberately stopped container within
        # seconds, so the debugging steps run on a second server over the same
        # state with the watcher off.
        srv.close()
        env_nowatch = dict(env, LTN_WATCH="0")
        srv = McpServer(exe, env_nowatch, os.path.join(work, "server1b.stderr.txt"))
        srv.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "ltn-e2e", "version": "0"}})
        srv.notify("notifications/initialized")

        def s_diagnose():
            a = app_status(srv, app)
            container = current_container(a, app)
            require(container, "cannot tell which container is current")
            docker("stop", "-t", "1", container, check=True)
            d = srv.call_ok("ltn_diagnose", {"app": app})
            findings = d.get("findings", [])
            require(findings, f"no findings for a stopped container: {json.dumps(d)[:500]}")
            for f in findings:
                for k in ("severity", "cause", "evidence", "suggested_action"):
                    require(k in f, f"finding lacks {k}: {f}")
            return f"verdict {d.get('verdict')}, {len(findings)} finding(s): {findings[0].get('cause')}"
        step("ltn_diagnose on stopped container")(s_diagnose)

        def s_rollback():
            r = srv.call_ok("ltn_rollback", {"app": app}, timeout=300)
            for k in ("app", "release", "from_release", "health"):
                require(k in r, f"rollback result lacks {k}")
            wait_for_page(state["endpoint"], "v1")
            return f"release {r['from_release']} -> {r['release']}"
        step("ltn_rollback -> v1")(s_rollback)

        def s_destroy():
            is_err, text, _ = srv.call("ltn_destroy", {"app": app})
            require(is_err, f"destroy without confirm succeeded: {text[:300]}")
            is_err, text, _ = srv.call("ltn_destroy", {"app": app, "confirm": "wrong-name"})
            require(is_err, f"destroy with wrong confirm succeeded: {text[:300]}")
            d = srv.call_ok("ltn_destroy", {"app": app, "confirm": app}, timeout=180)
            left = docker("ps", "-a", "--filter", f"label=ltn.app={app}", "--format", "{{.Names}}", check=True).stdout.split()
            require(not left, f"containers remain: {left}")
            return f"removed {d.get('removed_containers')}"
        step("ltn_destroy needs confirm, removes containers")(s_destroy)

        srv.close()
        srv = None

        env_nodocker = dict(env, LTN_WATCH="0", LTN_HOME=os.path.join(work, "home-nodocker"),
                            LTN_DOCKER_BIN=os.path.join(work, "no-such-dir", "docker.exe"))
        srv2 = McpServer(exe, env_nodocker, os.path.join(work, "server2.stderr.txt"))
        srv2.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "ltn-e2e", "version": "0"}})
        srv2.notify("notifications/initialized")

        def s_no_docker():
            is_err, text, p = srv2.call("ltn_plan", {"path": v1_nodocker})
            if is_err:
                return f"ltn_plan isError: {text[:160]}"
            is_err, text, _ = srv2.call("ltn_apply", {"plan_id": p["plan_id"]}, timeout=120)
            require(is_err, f"apply succeeded with no Docker binary: {text[:300]}")
            return f"ltn_apply isError: {text[:160]}"
        step("missing Docker -> plan/apply isError")(s_no_docker)

    finally:
        for s in (srv, srv2):
            if s is not None:
                try:
                    s.close()
                except Exception:  # noqa: BLE001
                    pass
        cleanup(app, app_nodocker)
        if all(ok for _, ok in RESULTS):
            shutil.rmtree(work, ignore_errors=True)
        else:
            print(f"info: kept {work} (server stderr logs) for inspection", flush=True)

    failed = [n for n, ok in RESULTS if not ok]
    print(f"SUMMARY {len(RESULTS) - len(failed)}/{len(RESULTS)} steps passed" + (f"; failed: {failed}" if failed else ""), flush=True)
    return 1 if failed else 0


def current_container(status_entry, app):
    """The current release's container: from ltn_status, else by convention."""
    if status_entry.get("container_name"):
        return status_entry["container_name"]
    if status_entry.get("release"):
        return f"ltn-{app}-r{status_entry['release']}"
    return container_for(app)


def container_for(app):
    """Fallback: the running container labelled for this app."""
    r = docker("ps", "--filter", f"label=ltn.app={app}", "--format", "{{.Names}}")
    names = r.stdout.split()
    return names[0] if names else None


def cleanup(*apps):
    for a in apps:
        try:
            names = docker("ps", "-aq", "--filter", f"label=ltn.app={a}").stdout.split()
            if names:
                docker("rm", "-f", *names, timeout=120)
            images = docker("images", "-q", f"ltn/{a}").stdout.split()
            if images:
                docker("rmi", "-f", *sorted(set(images)), timeout=120)
        except Exception as e:  # noqa: BLE001
            print(f"info: cleanup of {a} failed: {e}", flush=True)


if __name__ == "__main__":
    sys.exit(main())
