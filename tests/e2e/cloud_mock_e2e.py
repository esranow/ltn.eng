#!/usr/bin/env python3
"""Cloud path of `ltn mcp` against a LOCAL AWS MOCK (no AWS account, no spend).

Usage: python tests/e2e/cloud_mock_e2e.py [path/to/ltn.exe]   (default target/debug/ltn.exe)

What is real here: the ltn binary and its MCP handlers, the AWS CLI, OpenTofu
and the hashicorp/aws provider (init/apply/destroy really run), ssh-keygen,
the public-IP lookup. What is NOT real: AWS itself. Every AWS call goes to the
`nahuelnucera/ministack` container on 127.0.0.1:18500 with the fake key
`testing`, and `aws pricing get-products` (which the mock does not implement)
is answered by a wrapper with MADE-UP prices in the documented envelope. The
mock creates no VM, so provisioning MUST fail at the Docker-over-SSH wait; the
test asserts that ltn reports that, tears down, and registers nothing.

Needs: Docker running, `aws` and `python` on PATH, OpenTofu (LTN_TOFU_BIN or
.tools/tofu/tofu.exe), the Z3 DLL on PATH. Standard library only.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import mcp_e2e as base  # noqa: E402 - reuse the MCP stdio client and step helpers

REPO = base.REPO
PORT = 18500
ENDPOINT = f"http://127.0.0.1:{PORT}"
MOCK_NAME = "ltn-mock-aws"
MOCK_IMAGE = "nahuelnucera/ministack:latest"
# LTN_CLOUD_MOCK_EXTERNAL=1: a mock is already listening on ENDPOINT (e.g. `moto_server -H 127.0.0.1 -p 18500`
# from `pip install "moto[server]"`); the script then neither starts nor removes the Docker container.
EXTERNAL = os.environ.get("LTN_CLOUD_MOCK_EXTERNAL") == "1"
AL2023 = "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64"
# Made-up prices (USD/hour) for the instance types the mock describes.
MOCK_PRICES = {"t3.micro": "0.0104000000", "t3.small": "0.0208000000", "t3.medium": "0.0416000000",
               "t3.large": "0.0832000000", "m5.large": "0.0960000000", "m5.xlarge": "0.1920000000"}

WRAPPER_PY = """
import json, os, subprocess, sys
args = sys.argv[1:]
if args[:2] == ["pricing", "get-products"]:
    region = "us-east-1"
    for a in args:
        if "Field=regionCode" in a:
            region = a.split("Value=")[-1]
    items = []
    for it, usd in json.loads(os.environ["LTN_MOCK_PRICES"]).items():
        sku = "MOCK" + it.replace(".", "").upper()
        items.append(json.dumps({
            "product": {"productFamily": "Compute Instance", "sku": sku, "attributes": {
                "instanceType": it, "operatingSystem": "Linux", "tenancy": "Shared",
                "preInstalledSw": "NA", "capacitystatus": "Used", "regionCode": region}},
            "serviceCode": "AmazonEC2",
            "terms": {"OnDemand": {sku + ".JRTCKXETXF": {"priceDimensions": {sku + ".JRTCKXETXF.6YS6EN2CT7": {
                "unit": "Hrs", "pricePerUnit": {"USD": usd}, "description": "MOCK price"}}}}}}))
    print(json.dumps({"PriceList": items}))
    sys.exit(0)
sys.exit(subprocess.call([os.environ["LTN_REAL_AWS"]] + args))
"""


def sh(args, env=None, timeout=120):
    return subprocess.run(args, capture_output=True, text=True, env=env, timeout=timeout)


def assert_mock_only(env):
    """Hard guard: the spend switch may only ever be set against the local mock with fake keys."""
    assert env.get("AWS_ENDPOINT_URL", "").startswith("http://127.0.0.1"), "AWS_ENDPOINT_URL is not the local mock"
    assert env.get("AWS_ACCESS_KEY_ID") == "testing", "access key is not the fake `testing` key"
    assert env.get("AWS_SECRET_ACCESS_KEY") == "testing", "secret key is not the fake `testing` key"


def main():
    exe = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(REPO, "target", "debug", "ltn.exe"))
    tofu = os.environ.get("LTN_TOFU_BIN") or os.path.join(REPO, ".tools", "tofu", "tofu.exe")
    real_aws = shutil.which("aws")
    for what, p in (("ltn binary", exe), ("OpenTofu", tofu), ("aws CLI", real_aws)):
        if not p or not os.path.isfile(p):
            print(f"FAIL setup -- {what} not found ({p})")
            return 2

    work = tempfile.mkdtemp(prefix="ltn-cloudmock-")
    home = os.path.join(work, "home")
    os.makedirs(home)
    with open(os.path.join(work, "aws_mock.py"), "w", encoding="utf-8") as f:
        f.write(WRAPPER_PY)
    wrapper = os.path.join(work, "aws_mock.cmd")
    with open(wrapper, "w", encoding="utf-8", newline="\r\n") as f:
        f.write(f'@"{sys.executable}" "%~dp0aws_mock.py" %*\n')
    app = f"cloudmock-{uuid.uuid4().hex[:6]}"
    app_dir = base.copy_app("web-v1", work, app, 18510)

    env = dict(os.environ)
    for k in ("AWS_PROFILE", "AWS_SESSION_TOKEN", "LTN_ALLOW_CLOUD_SPEND", "LTN_MAX_HOURLY_CENTS", "LTN_DOCKER_BIN"):
        env.pop(k, None)
    env.update({
        "AWS_ENDPOINT_URL": ENDPOINT, "AWS_ACCESS_KEY_ID": "testing", "AWS_SECRET_ACCESS_KEY": "testing",
        "AWS_DEFAULT_REGION": "us-east-1", "AWS_REGION": "us-east-1", "AWS_EC2_METADATA_DISABLED": "true",
        "AWS_CONFIG_FILE": os.path.join(work, "no-aws-config"),
        "AWS_SHARED_CREDENTIALS_FILE": os.path.join(work, "no-aws-credentials"),
        "LTN_HOME": home, "LTN_WATCH": "0", "LTN_AWS_BIN": wrapper, "LTN_REAL_AWS": real_aws,
        "LTN_MOCK_PRICES": json.dumps(MOCK_PRICES), "LTN_TOFU_BIN": tofu,
        "LTN_SSH_CONFIG": os.path.join(work, "user-ssh", "config"), "LTN_CLOUD_DOCKER_WAIT_S": "12",
        "TF_PLUGIN_CACHE_DIR": os.environ.get("TF_PLUGIN_CACHE_DIR", r"C:\temp\ltn-tofu-plugin-cache"),
    })
    os.makedirs(env["TF_PLUGIN_CACHE_DIR"], exist_ok=True)
    assert_mock_only(env)
    print(f"info: exe={exe} work={work} app={app}", flush=True)

    servers = []
    st = {}

    def server(extra, tag):
        e = dict(env)
        e.update(extra)
        if e.get("LTN_ALLOW_CLOUD_SPEND"):
            assert_mock_only(e)
        s = base.McpServer(exe, e, os.path.join(work, f"{tag}.stderr.txt"))
        servers.append(s)
        s.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                 "clientInfo": {"name": "cloud-mock-e2e", "version": "0"}})
        s.notify("notifications/initialized")
        return s

    def aws(*a):
        return sh([real_aws, *a, "--output", "json"], env=env)

    try:
        def s_mock():
            if not EXTERNAL:
                sh(["docker", "rm", "-f", MOCK_NAME])
                r = sh(["docker", "run", "-d", "--name", MOCK_NAME, "-p", f"127.0.0.1:{PORT}:4566", MOCK_IMAGE])
                base.require(r.returncode == 0, f"could not start the mock: {r.stderr[-300:]}")
            for _ in range(30):
                if aws("sts", "get-caller-identity").returncode == 0:
                    break
                time.sleep(1)
            else:
                raise base.StepFailed("mock did not answer sts within 30s")
            r = aws("ssm", "put-parameter", "--name", AL2023, "--value", "ami-0abcdef1234567890",
                    "--type", "String", "--overwrite")
            if r.returncode != 0:  # moto ships the public /aws/service parameters and refuses to overwrite them
                g = aws("ssm", "get-parameter", "--name", AL2023)
                base.require(g.returncode == 0, f"no AL2023 AMI parameter in the mock: {r.stderr[-200:]} {g.stderr[-200:]}")
            return f"mock on {ENDPOINT}"
        if not base.step("start local AWS mock")(s_mock):
            return 1

        a = server({}, "srv-default")

        def s_auth():
            v = a.call_ok("ltn_cloud_auth")
            base.require(v.get("configured") is True and v.get("account") == "000000000000", json.dumps(v))
            return f"account {v['account']} (mock)"
        base.step("ltn_cloud_auth sees the (mock) identity")(s_auth)

        def s_catalog():
            v = a.call_ok("ltn_cloud_catalog", {"refresh": True, "region": "us-east-1", "families": ["t3", "m5"]})
            base.require(v.get("source") == "aws" and v.get("count", 0) > 0, json.dumps(v)[:500])
            e = {x["instance_type"]: x for x in v["entries"]}
            base.require("t3.small" in e and e["t3.small"]["cost_cents_per_hour"] == 3, json.dumps(e.get("t3.small")))
            st["offer"] = e["t3.small"]["offer_id"]
            c = a.call_ok("ltn_cloud_catalog", {})
            base.require(c.get("source") == "cache" and c.get("count") == v["count"], json.dumps(c)[:300])
            return f"{v['count']} entries, offer {st['offer']}, unpriced {v.get('unpriced')}"
        base.step("ltn_cloud_catalog refresh (mock types, wrapper prices) + cache")(s_catalog)

        def s_plan():
            v = a.call_ok("ltn_plan", {"path": app_dir, "target": st["offer"]})
            base.require(v.get("provision_offer") == st["offer"], json.dumps(v)[:500])
            st["plan"] = v["plan_id"]
            return f"plan {v['plan_id'][:8]} carries provision_offer"
        base.step("ltn_plan pinned to the offer carries provision_offer")(s_plan)

        def s_refuse():
            is_err, text, _ = a.call("ltn_apply", {"plan_id": st["plan"]})
            base.require(is_err and "LTN_ALLOW_CLOUD_SPEND" in text, text[:500])
            base.require(not os.path.isdir(os.path.join(home, "cloud")), "a cloud module dir exists although apply was refused")
            t = a.call_ok("ltn_cloud_targets")
            base.require(t.get("count") == 0, json.dumps(t))
            return "refused, nothing created"
        base.step("ltn_apply refuses without LTN_ALLOW_CLOUD_SPEND")(s_refuse)
        a.close()

        b = server({"LTN_MAX_HOURLY_CENTS": "1"}, "srv-budget")

        def s_budget():
            is_err, text, _ = b.call("ltn_plan", {"path": app_dir, "target": st["offer"]})
            base.require(is_err and "budget" in text, text[:600])
            return text.strip().splitlines()[0][:160]
        base.step("plan over LTN_MAX_HOURLY_CENTS is unsatisfiable with `budget` in the core")(s_budget)
        b.close()

        c = server({"LTN_ALLOW_CLOUD_SPEND": "1"}, "srv-spend")

        def s_apply():
            v = c.call_ok("ltn_plan", {"path": app_dir, "target": st["offer"]})
            t0 = time.time()
            is_err, text, _ = c.call("ltn_apply", {"plan_id": v["plan_id"]}, timeout=600)
            st["apply_text"] = text
            base.require(is_err, f"apply reported success although the mock has no VM: {text[:600]}")
            base.require("did not become reachable" in text, f"failure is not the Docker/SSH wait: {text[:1500]}")
            base.require("cleanup succeeded" in text, f"teardown did not succeed: {text[:1500]}")
            return (f"failed honestly after {time.time() - t0:.0f}s: tofu init/apply ran, "
                    "Docker wait timed out, tofu destroy ran")
        base.step("ltn_apply with spend switch: real tofu apply on mock, honest failure, teardown")(s_apply)

        def s_clean():
            t = c.call_ok("ltn_cloud_targets")
            base.require(t.get("count") == 0, f"infra record left behind: {json.dumps(t)}")
            ts = c.call_ok("ltn_targets")
            ids = [x.get("id") for x in ts.get("targets", [])]
            base.require(all(not str(i).startswith(app) for i in ids), f"half-registered target: {ids}")
            cloud = os.path.join(home, "cloud")
            left = os.listdir(cloud) if os.path.isdir(cloud) else []
            base.require(not left, f"module dirs left: {left}")
            cfg = os.path.join(home, "ssh_config")
            base.require(not os.path.isfile(cfg) or "Host ltn-" not in open(cfg, encoding="utf-8").read(),
                         "ssh alias left behind")
            r = aws("ec2", "describe-instances", "--query", "Reservations[].Instances[].State.Name")
            states = json.loads(r.stdout or "[]")
            base.require(all(s in ("terminated", "shutting-down") for s in states), f"mock instances still alive: {states}")
            au = c.call_ok("ltn_audit", {})
            base.require("cloud_provision_failed" in json.dumps(au), "audit has no cloud_provision_failed entry")
            return f"no record, no target, no module dir, mock instance states {states}, audit has cloud_provision_failed"
        base.step("nothing half-registered after the failed provision")(s_clean)
        c.close()
    finally:
        for s in servers:
            try:
                s.close()
            except Exception:  # noqa: BLE001
                pass
        if not EXTERNAL:
            sh(["docker", "rm", "-f", MOCK_NAME])
        if st.get("apply_text") and any(not ok for _, ok in base.RESULTS):
            print("apply error text:\n" + st["apply_text"][:3000])

    failed = [n for n, ok in base.RESULTS if not ok]
    print(f"\n{len(base.RESULTS) - len(failed)}/{len(base.RESULTS)} steps passed; work dir {work}")
    if not failed:
        shutil.rmtree(work, ignore_errors=True)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
