#!/usr/bin/env python3
"""End-to-end test of the on-prem SSH path and multi-node placement, on one machine.

Usage: python tests/e2e/remote_e2e.py [path/to/ltn.exe] [--keep]

Starts two real "remote" Docker hosts as containers (tests/e2e/remote-host:
a Docker daemon behind sshd, published on 127.0.0.1 only), registers them with
`ltn_target_add` as `ssh://root@127.0.0.1:<port>` and drives `ltn mcp` over stdio.
The app's container runs on the remote daemon, not the local one.

Nothing in ~/.ssh is read or written: a throwaway key, ssh config and known_hosts
live in a temp dir, and a tiny `ssh` shim that adds `-F <that config>` is put first
on PATH for the ltn process only (the docker CLI runs `ssh` from PATH).

Needs: Docker running, an OpenSSH client, a built ltn binary (Z3 DLL on PATH).
Ports used: 18411-18412 (apps), 18422/18424 (sshd), 18449 (must be closed).
Standard library only. Exits nonzero on any failed step.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mcp_e2e import (  # noqa: E402
    HERE, REPO, RESULTS, McpServer, StepFailed, app_status, copy_app, require, step, wait_for_page,
)

HOST_IMAGE = "ltn-e2e-remote-host"
NODES = {
    # id: (container, ssh port, app host port, cost cents/h, gpu_count, vram_mb)
    "node-a": ("ltn-e2e-node-a", 18422, 18411, 5, 1, 16000),
    "node-b": ("ltn-e2e-node-b", 18424, 18412, 2, 1, 8000),
}
WIN = os.name == "nt"


def tool(name):
    if WIN:
        p = os.path.join(os.environ.get("SystemRoot", r"C:\Windows"), "System32", "OpenSSH", name + ".exe")
        if os.path.isfile(p):
            return p
    p = shutil.which(name)
    if not p:
        raise SystemExit(f"FAIL setup -- {name} not found")
    return p


def run(cmd, env=None, timeout=120, check=True):
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, env=env)
    if check and r.returncode != 0:
        raise StepFailed(f"{' '.join(cmd)[:160]} failed: {(r.stderr or r.stdout).strip()[:400]}")
    return r


def host_url(node):
    return f"ssh://root@127.0.0.1:{NODES[node][1]}"


def set_toml(app_dir, **kv):
    """Append/replace simple integer keys in the app's ltn.toml."""
    path = os.path.join(app_dir, "ltn.toml")
    with open(path, encoding="utf-8") as f:
        lines = [ln for ln in f.read().splitlines() if ln.split("=")[0].strip() not in kv]
    lines += [f"{k} = {v}" for k, v in kv.items()]
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")


def oracle(considered, cpu, ram, gpu, vram, budget):
    """Cheapest-fit expectation computed independently of the solver."""
    fits = [c for c in considered
            if c["free_cpu_millis"] >= cpu and c["free_ram_mb"] >= ram and c["free_gpu_count"] >= gpu
            and c["free_vram_mb"] >= vram and (budget is None or c["cost_cents_per_hour"] <= budget)]
    if not fits:
        return set(), None
    low = min(c["cost_cents_per_hour"] for c in fits)
    return {c["target_id"] for c in fits if c["cost_cents_per_hour"] == low}, low


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    keep = "--keep" in sys.argv
    exe = os.path.abspath(args[0] if args else os.path.join(REPO, "target", "debug", "ltn.exe"))
    if not os.path.isfile(exe):
        print(f"FAIL setup -- {exe} not found; build it with `cargo build -p ltn-cli`")
        return 2

    suffix = uuid.uuid4().hex[:8]
    app = f"e2e-remote-{suffix}"
    gpu_app = f"e2e-gpu-{suffix}"
    work = tempfile.mkdtemp(prefix="ltn-remote-e2e-")
    home = os.path.join(work, "home")
    shim = os.path.join(work, "shim")
    os.makedirs(home)
    os.makedirs(shim)
    key = os.path.join(work, "id_ed25519")
    cfg = os.path.join(work, "ssh_config")
    ssh = tool("ssh")

    env = dict(os.environ)
    env["PATH"] = shim + os.pathsep + env.get("PATH", "")
    env.update({"LTN_HOME": home, "LTN_WATCH_INTERVAL_S": "2"})
    env.pop("LTN_WATCH", None)
    env.pop("LTN_DOCKER_BIN", None)
    env.pop("DOCKER_HOST", None)

    def rdocker(node, *a, check=True, timeout=120):
        return run(["docker", "-H", host_url(node), *a], env=env, timeout=timeout, check=check)

    print(f"info: exe={exe} app={app} work={work}", flush=True)
    srv = None
    state = {}
    try:
        def s_setup():
            run([tool("ssh-keygen"), "-t", "ed25519", "-N", "", "-q", "-C", "ltn-remote-e2e", "-f", key])
            with open(key + ".pub", encoding="utf-8") as f:
                pub = f.read().strip()
            with open(cfg, "w", encoding="utf-8") as f:
                f.write(f'Host *\n  IdentityFile "{key}"\n  IdentitiesOnly yes\n'
                        f'  UserKnownHostsFile "{os.path.join(work, "known_hosts")}"\n'
                        "  StrictHostKeyChecking accept-new\n  BatchMode yes\n")
            if WIN:
                with open(os.path.join(shim, "ssh.cmd"), "w", newline="\r\n") as f:
                    f.write(f'@echo off\n"{ssh}" -F "{cfg}" %*\n')
            else:
                p = os.path.join(shim, "ssh")
                with open(p, "w", newline="\n") as f:
                    f.write(f'#!/bin/sh\nexec "{ssh}" -F "{cfg}" "$@"\n')
                os.chmod(p, 0o755)
            run(["docker", "build", "-q", "-t", HOST_IMAGE, os.path.join(HERE, "remote-host")], timeout=600)
            for node, (cname, ssh_port, app_port, *_rest) in NODES.items():
                run(["docker", "rm", "-f", "-v", cname], check=False)
                run(["docker", "run", "-d", "--privileged", "--name", cname, "-e", f"LTN_E2E_PUBKEY={pub}",
                     "-p", f"127.0.0.1:{ssh_port}:22", "-p", f"127.0.0.1:{app_port}:{app_port}", HOST_IMAGE])
            for node in NODES:
                deadline, last = time.time() + 90, ""
                while True:
                    r = rdocker(node, "info", "--format", "{{.ServerVersion}}", check=False, timeout=60)
                    if r.returncode == 0 and r.stdout.strip():
                        break
                    last = (r.stderr or r.stdout).strip()[-300:]
                    require(time.time() < deadline, f"{node} daemon not reachable over ssh in 90s: {last}")
                    time.sleep(2)
            return f"2 dockerd+sshd nodes up, ssh client {ssh}"
        if not step("setup: two real Docker hosts behind sshd")(s_setup):
            return 1

        srv = McpServer(exe, env, os.path.join(work, "server.stderr.txt"))
        r = srv.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                       "clientInfo": {"name": "ltn-remote-e2e", "version": "0"}})
        require("result" in r, f"initialize failed: {r}")
        srv.notify("notifications/initialized")

        def s_add():
            out = []
            for node, (_c, _sp, _ap, cost, gpus, vram) in NODES.items():
                t = srv.call_ok("ltn_target_add", {"id": node, "docker_host": host_url(node), "address": "127.0.0.1",
                                                   "cost_cents_per_hour": cost, "gpu_count": gpus, "vram_mb": vram})
                require(t.get("reachable") is True, f"{node} not reachable: {t}")
                out.append(f"{node} cap={json.dumps(t.get('capacity'))[:120]}")
            ts = {t["id"]: t for t in srv.call_ok("ltn_targets")["targets"]}
            for node in NODES:
                require(ts.get(node, {}).get("reachable") is True, f"ltn_targets: {node} not reachable: {ts.get(node)}")
                require(ts[node].get("provider") == "ssh", f"{node} provider {ts[node].get('provider')}")
            return "; ".join(out)
        step("ltn_target_add two ssh hosts, ltn_targets shows them reachable")(s_add)

        v1 = copy_app("web-v1", work, app, NODES["node-a"][2])

        def s_apply():
            p = srv.call_ok("ltn_plan", {"path": v1, "target": "node-a"})
            require(p["target_id"] == "node-a", f"pinned plan chose {p['target_id']}")
            is_err, text, a = srv.call("ltn_apply", {"plan_id": p["plan_id"]}, timeout=420)
            require(not is_err, f"apply isError: {text[:900]}")
            require(a["target_id"] == "node-a", f"applied on {a['target_id']}")
            state["endpoint"], state["container"] = a["endpoint"], a["container"]
            return f"release {a['release']} container {a['container']} at {a['endpoint']}"
        step("ltn_plan + ltn_apply onto the ssh host")(s_apply)

        def s_verify_remote():
            require(state.get("container"), "no container from apply")
            names = rdocker("node-a", "ps", "--filter", f"label=ltn.app={app}", "--format", "{{.Names}}").stdout.split()
            require(names == [state["container"]], f"remote daemon runs {names}, expected [{state['container']}]")
            local = run(["docker", "ps", "-a", "--filter", f"label=ltn.app={app}", "--format", "{{.Names}}"]).stdout.split()
            require(not local, f"container also exists on the LOCAL daemon: {local}")
            other = rdocker("node-b", "ps", "-a", "--filter", f"label=ltn.app={app}", "--format", "{{.Names}}").stdout.split()
            require(not other, f"container also exists on node-b: {other}")
            wait_for_page(state["endpoint"], "v1")
            return f"only on node-a; {state['endpoint']} serves v1"
        step("container runs on the remote daemon only and answers HTTP")(s_verify_remote)

        def s_status():
            a = app_status(srv, app)
            require(a.get("verdict") == "healthy", f"verdict {a.get('verdict')}: {json.dumps(a)[:500]}")
            d = srv.call_ok("ltn_diagnose", {"app": app})
            lg = srv.call_ok("ltn_logs", {"app": app})
            require(str(lg.get("logs", "")).strip(), "empty logs from the remote container")
            return f"healthy; diagnose findings={len(d.get('findings', []))}; logs {len(lg['logs'])} chars"
        step("ltn_status / ltn_diagnose / ltn_logs read the remote host")(s_status)

        def s_watch():
            require(state.get("container"), "no container")
            killed_at = time.time()
            rdocker("node-a", "kill", state["container"])
            deadline, healthy, last = killed_at + 60, False, None
            while time.time() < deadline:
                time.sleep(1)
                last = app_status(srv, app)
                if last.get("verdict") == "healthy":
                    healthy = True
                    break
            require(healthy, f"not healthy within 60s of remote docker kill (last verdict {last and last.get('verdict')})")
            au = srv.call_ok("ltn_audit", {"app": app, "limit": 50})
            actions = [e.get("action", "") for e in au.get("entries", [])]
            watch = [x for x in actions if x.startswith("watch.") and not x.endswith("_failed")]
            require(watch, f"no watcher action in audit; actions: {actions}")
            wait_for_page(state["endpoint"], "v1", seconds=15)
            return f"healthy after {time.time() - killed_at:.1f}s; watcher actions {watch}"
        step("kill on the remote daemon -> watcher repairs it")(s_watch)

        gpu_dir = copy_app("web-v1", work, gpu_app, NODES["node-b"][2])
        probe_dir = gpu_dir + "-probe"
        shutil.copytree(gpu_dir, probe_dir)
        set_toml(probe_dir, max_cost_cents_per_hour=100000)

        def plan_case(label, gpu, vram, budget):
            kv = {"gpu": gpu, "vram_mb": vram}
            if budget is not None:
                kv["max_cost_cents_per_hour"] = budget
            set_toml(gpu_dir, **kv)
            # Capacity as the server sees it, from an unconstrained plan of the same app.
            considered = srv.call_ok("ltn_plan", {"path": probe_dir})["capacity_considered"]
            expected, low = oracle(considered, 250, 64, gpu, vram, budget)
            is_err, text, p = srv.call("ltn_plan", {"path": gpu_dir})
            if not expected:
                require(is_err, f"{label}: nothing fits but plan chose {p and p.get('target_id')}")
                first = (text.strip().splitlines() or [""])[0][:140]
                return f"{label}: refused ({first})"
            require(not is_err, f"{label}: expected one of {sorted(expected)} but plan failed: {text[:400]}")
            require(p["target_id"] in expected, f"{label}: chose {p['target_id']}, cheapest fit is {sorted(expected)}")
            require(p["cost_cents_per_hour"] == low, f"{label}: cost {p['cost_cents_per_hour']} != {low}")
            state.setdefault("chosen", []).append(p["target_id"])
            return f"{label}: chose {p['target_id']} at {low}c/h (of {len(considered)} targets)"

        def s_multi():
            out = [plan_case("1 GPU", 1, 0, None),
                   plan_case("1 GPU + 12000 MB VRAM", 1, 12000, None),
                   plan_case("1 GPU + 12000 MB VRAM, budget 3c/h", 1, 12000, 3),
                   plan_case("1 GPU + 64000 MB VRAM", 1, 64000, None)]
            require(len(set(state.get("chosen", []))) >= 2,
                    f"constraints never changed the choice (chosen: {state.get('chosen')}); the test proved nothing")
            return " | ".join(out)
        step("multi-node: Z3 picks the cheapest host that fits, constraints change the choice")(s_multi)

        def s_unreachable():
            started = time.time()
            is_err, text, _ = srv.call("ltn_target_add", {"id": "ghost", "docker_host": "ssh://root@127.0.0.1:18449",
                                                          "address": "127.0.0.1"}, timeout=120)
            require(is_err, f"unreachable target was added: {text[:300]}")
            ids = [t["id"] for t in srv.call_ok("ltn_targets")["targets"]]
            require("ghost" not in ids, f"ghost is listed: {ids}")
            first = (text.strip().splitlines() or [""])[0][:160]
            return f"rejected in {time.time() - started:.1f}s: {first}"
        step("unreachable ssh target is rejected and not saved")(s_unreachable)

        def s_destroy():
            is_err, text, _ = srv.call("ltn_target_remove", {"id": "node-a"})
            require(is_err, f"removed a target that still hosts an app: {text[:200]}")
            srv.call_ok("ltn_destroy", {"app": app, "confirm": app}, timeout=180)
            left = rdocker("node-a", "ps", "-a", "--filter", f"label=ltn.app={app}", "--format", "{{.Names}}").stdout.split()
            require(not left, f"containers remain on node-a: {left}")
            for node in NODES:
                t = srv.call_ok("ltn_target_remove", {"id": node})
                require(t.get("removed") is True, f"{node} not removed: {t}")
            ids = [t["id"] for t in srv.call_ok("ltn_targets")["targets"]]
            require(ids == ["local"], f"targets after removal: {ids}")
            return "app gone from node-a, both targets removed"
        step("ltn_destroy on the remote host, ltn_target_remove")(s_destroy)
    except StepFailed as e:
        print(f"FAIL harness -- {e}", flush=True)
        RESULTS.append(("harness", False))
    finally:
        if srv is not None:
            srv.close()
        if keep:
            print(f"info: --keep: nodes and {work} left in place", flush=True)
        else:
            for cname, *_ in NODES.values():
                subprocess.run(["docker", "rm", "-f", "-v", cname], capture_output=True)
            subprocess.run(["docker", "rmi", HOST_IMAGE], capture_output=True)
            shutil.rmtree(work, ignore_errors=True)

    failed = [n for n, ok in RESULTS if not ok]
    print(f"\n{len(RESULTS) - len(failed)}/{len(RESULTS)} steps passed" + (f"; FAILED: {failed}" if failed else ""), flush=True)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
