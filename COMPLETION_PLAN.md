# ltn completion plan

## EXECUTOR PROMPT (paste this to the model that will finish the work)

You are finishing the `ltn` product in `D:\ltn-eng-workspace`. Read this whole
file first. Then do the TASKS below in order, one at a time.

For every task:
1. Run exactly the command given, in the shell named (PowerShell or Git Bash).
2. Compare the output with "Expect" and apply the "Pass check".
3. If it passes, change its `[ ]` to `[x]` in this file and add one line under it
   saying what you saw (counts, hashes). Commit this file with the task's changes.
4. If it fails, follow "If it fails". If the same check fails twice, STOP and
   report to the user with the exact output. Do not guess or skip ahead.

Never push, merge, tag, publish or release. Those are the user's commands in
section 7. Never say something passed unless you ran it and saw it pass.
Report at the end: tasks ticked, tasks failed (with output), what is left.

---

## 1. GOAL: "done" means

- [ ] `cargo test --workspace` passes (0 failed) on branch `sprint/real-deploy-path`.
- [ ] The Docker-gated tests pass: `ltn-engine`, `ltn-observe`, `ltn-mcp`.
- [ ] `tests/e2e/mcp_e2e.py` passes all steps on real Docker.
- [ ] `tests/e2e/adk_connect.py` passes (Google ADK lists 18 tools and calls them).
- [ ] The wip fixes (SSH shim, map-mtl bridge, duplicate bin removal, e2e timeout scale) are verified and merged into `sprint/real-deploy-path`.
- [ ] `tests/e2e/remote_e2e.py` (SSH Docker hosts, multi-node) passes, or its failure is written down with the reason.
- [ ] `tests/e2e/cloud_mock_e2e.py` (AWS mock, fake keys) passes, or its failure is written down.
- [ ] Release archive rebuilt from the final commit: `dist\ltn-0.1.0-x86_64-pc-windows-msvc.zip` + `dist\SHA256SUMS`.
- [ ] README "Status and limits" matches what was actually verified.

## 2. CURRENT STATE (2026-11-20, 09:24 local)

Repository: `D:\ltn-eng-workspace`. No git remote exists.

Branches:
- `sprint/real-deploy-path` — the product. HEAD `e022849` (this plan is committed on top).
  Earlier: `567dee2` (LF for .sh/Dockerfile), `7fd126c` (Docker timeouts, ADK test, packaging), `de267be` (18-tool MCP runtime).
- `wip/fundamental-fixes` — commit `bd72892`, on top of `e022849`. Stopped mid-run. NOT verified as a whole.
- `master` — untouched baseline. Do not touch.

Verified (I ran these myself):
- Base crate `ltn-state`: 11/11 tests, including Docker call timeouts (`LTN_DOCKER_TIMEOUT_S`, default 120 s).
- `ltn-mcp`: 19 passed, 1 ignored. Tool calls run concurrently (cap 8). `ltn_doctor` returns within 25 s, even when Docker hangs.
- `ltn-observe`: 30/30 unit tests. `ltn_repair` and the watcher's start/restart now hold the per-app lock.
- ADK: `tests/e2e/adk_connect.py` PASSED with google-adk 2.15.0 (before the concurrency change; re-run in task 6).
- Release binary `target\release\ltn.exe` built at 09:00 from `567dee2`: `ltn 0.1.0`, 18 tools over MCP. Archive SHA-256 `2e147bec3256e207614ca2beaf8db30dfe7f6f64fc738019e83dd1ae333223a5`. It predates `e022849`, so rebuild it (task 12).
- Earlier sprint (`de267be`): workspace 447 passed / 0 failed / 46 ignored; e2e 16/16 on an idle machine.
- map-mtl: 35/35 tests on Linux (Alpine, WSL2 kernel) and on Windows. Standalone repository at `D:\map-mtl-public`, commit `cde9ad9`, not pushed. `OWNER` placeholders are at `Cargo.toml:8` and `README.md:86`.

Partial or untested:
- `wip/fundamental-fixes` (`bd72892`):
  - `crates/ltn-state/src/ssh.rs`: ltn's own ssh wrapper, so `docker -H ssh://` works without touching `~/.ssh`.
  - `targets.rs`: `identity_file` argument.
  - `sshcfg.rs`: one shared ssh_config.
  - `map_mtl_bridge.rs`: current schema, errors instead of a made-up topology.
  - `standalone_main.rs`: the duplicate `map-mtl` bin is removed.
  - `LTN_E2E_TIMEOUT_SCALE` in the e2e scripts.
  - Only `ltn-core` (109/109) was reported passing. The rest has not been run on this tree.
- `remote_e2e.py`: 0 passing runs. SSH into a docker:dind node was proven by hand; setup timed out under memory pressure.
- `cloud_mock_e2e.py`: 0 passing runs. The mock container could not start under memory pressure; moto (no Docker) was never installed.
- `mcp_e2e.py` this morning: 6 passed, 6 failed. All failures were Docker reply timeouts with about 400 MB of free RAM, not logic errors.

Never run: a real AWS account, a real remote SSH host, an LLM driving the tools.

## 3. TASKS

Z3 environment, needed by every cargo command (Git Bash):
```
export Z3_SYS_Z3_HEADER='D:\beam\z3-bin\z3-4.12.2-x64-win\include\z3.h' Z3_DIR='D:\beam\z3-bin\z3-4.12.2-x64-win' BINDGEN_EXTRA_CLANG_ARGS='-ID:\beam\z3-bin\z3-4.12.2-x64-win\include' PATH="/d/beam/z3-bin/z3-4.12.2-x64-win/bin:$PATH" CARGO_BUILD_JOBS=2
```
RAM check (PowerShell):
```
[int]((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory/1024)
```

- [ ] **T1. Check the machine.** PowerShell: run the RAM check and `docker info --format '{{.NCPU}}'`.
  Expect: free RAM in MB, and `8`. Pass: RAM > 2500 and docker answers within 10 s.
  If it fails: close other heavy programs or start Docker Desktop, wait, retry. If it still fails, do only T2-T5 (no Docker) and report.
- [ ] **T2. Check out the wip branch.** Git Bash: `cd /d/ltn-eng-workspace && git checkout wip/fundamental-fixes && git log --oneline -1` (this branch does not contain COMPLETION_PLAN.md; tick boxes in the copy on `sprint/real-deploy-path` after T5).
  Expect: `bd72892 wip: ...`. Pass: that hash is printed.
- [ ] **T3. Build.** Git Bash (Z3 env): `cargo build -p ltn-cli 2>&1 | tail -3`.
  Expect: `Finished`. Pass: no `error`.
  If it fails: read the first error and fix only that file. Likely places are `crates/ltn-state/src/ssh.rs`, `tool.rs`, `docker.rs` and `crates/ltn-cli/Cargo.toml`.
- [ ] **T4. Unit tests of the touched crates.** Git Bash (Z3 env): `cargo test -p ltn-state -p ltn-engine -p ltn-cloud -p ltn-core -p ltn-mcp -p ltn-observe -p ltn-cli 2>&1 | grep -E "test result|FAILED|panicked"`.
  Pass: every `test result` line says `0 failed`. If it fails: fix the failing test's cause, not the test's assertion.
- [ ] **T5. Whole workspace.** Git Bash (Z3 env): `cargo test --workspace 2>&1 | grep -E "test result|FAILED" | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}'`.
  Pass: `0 failed`. Then merge the wip work: `git checkout sprint/real-deploy-path && git merge --no-edit wip/fundamental-fixes`. This is local only; never push.
- [ ] **T6. ADK check.** Needs a venv with `pip install google-adk mcp` (python 3.12). Git Bash: `cp target/debug/ltn.exe /tmp/ltn-adk.exe; LTN_BIN="$(cygpath -w /tmp/ltn-adk.exe)" LTN_MAP_MTL_BIN='D:\ltn-eng-workspace\bin\map-mtl.exe' <venv>/Scripts/python.exe tests/e2e/adk_connect.py`.
  Expect: last line `ADK CONNECT: PASSED`.
- [ ] **T7. Docker-gated tests** (needs T1 to pass). Git Bash (Z3 env): `for c in ltn-engine ltn-observe ltn-mcp; do cargo test -p $c -- --ignored --test-threads=1 2>&1 | grep "test result"; done`.
  Pass: 0 failed for all three (earlier: 5, 13, 1).
- [ ] **T8. Main e2e** (needs T1). Git Bash: `cp target/debug/ltn.exe /tmp/ltn-e2e.exe; PATH="/d/beam/z3-bin/z3-4.12.2-x64-win/bin:$PATH" LTN_E2E_TIMEOUT_SCALE=2 python tests/e2e/mcp_e2e.py "$(cygpath -w /tmp/ltn-e2e.exe)"`.
  Expect: every line `PASS` (16 steps). If a step times out: check RAM; re-run once when RAM > 2500.
- [ ] **T9. Remote SSH e2e** (needs T1; run alone, no other Docker job). Git Bash: `python tests/e2e/remote_e2e.py "$(cygpath -w /tmp/ltn-e2e.exe)"`.
  Expect: all steps PASS. If setup times out twice, record it under this task and move on.
- [ ] **T10. Cloud mock e2e.** Docker-free: `pip install "moto[server]"` in the venv, then `LTN_CLOUD_MOCK=moto python tests/e2e/cloud_mock_e2e.py "$(cygpath -w /tmp/ltn-e2e.exe)"`.
  Expect: all steps PASS. The script asserts the endpoint is 127.0.0.1 and the key is `testing` before it sets `LTN_ALLOW_CLOUD_SPEND`. Never remove that assertion.
- [ ] **T11. README truth pass.** Edit the "Status and limits" section of `README.md` to match T4-T10 exactly: what passed, what did not, and why. Commit.
- [ ] **T12. Release build and archive.** Git Bash (Z3 env): `cargo build --release -p ltn-cli --bin ltn && ./target/release/ltn.exe --version && python scripts/package_release.py && cat dist/SHA256SUMS`.
  Expect: `ltn 0.1.0`, a zip path and a hash. Record the hash here.
- [ ] **T13. Final commit** of this file with all ticks: `git add COMPLETION_PLAN.md README.md && git commit -m "docs: completion plan results"`. Do not push.

## 4. ORDER

T1 first. Then T2 → T3 → T4 → T5, in order. T6 and T10 do not need Docker and can run in parallel with T7.

Run T7, T8 and T9 one at a time; they all use Docker. Then T11 → T12 → T13.

## 5. STOP RULES (stop and ask the user)

- Before any `git push`, merge to `master`, `git tag`, `gh release`, `cargo publish` or deploy.
- When the same check fails twice.
- Before setting `LTN_ALLOW_CLOUD_SPEND=1`, except inside `cloud_mock_e2e.py`, which only runs against the local mock with fake keys.
- If anything asks for real cloud credentials, an SSH host or a GitHub account.

## 6. TRAPS

- **Low RAM stalls Docker Desktop.** Below about 1 GB free, `docker info` takes over 40 s and e2e steps time out. Check RAM before every Docker task, and run Docker jobs one at a time.
- **Do not run `target\debug\ltn.exe` directly in tests.** A rebuild cannot replace a running exe on Windows; copy it first.
- **`libz3.dll` must be on PATH** (`D:\beam\z3-bin\z3-4.12.2-x64-win\bin`), or ltn will not start.
- **Do not write to `~/.ssh` or `~/.aws`.** ltn keeps its own ssh_config and known_hosts under `LTN_HOME`.
- **Do not touch** `D:\ai_projects\Rippl-*` (another session), `master`, or `D:\ai_projects` git history.
- **map-mtl** source at `D:\ai_projects\map-mtl` is untracked in that repo. The committed copy is `D:\map-mtl-public`.
- **Legacy crates** (`ltn-agent`, `ltn-ops`, `ltn-providers`, `ltn-container`) are demos. 26 of their tests are `#[ignore]`d with reasons; leave them ignored.
- **LF/CRLF warnings** from git are harmless. `.gitattributes` keeps `*.sh` and `Dockerfile` as LF.
- **ltn has no licence file.** That is the user's decision; do not add one.

## 7. FINAL RELEASE COMMANDS (USER RUNS THIS, not the model)

```
# user runs this, after choosing a licence and creating the GitHub repo
cd D:\ltn-eng-workspace
git remote add origin https://github.com/<OWNER>/ltn.git
git push -u origin sprint/real-deploy-path
gh release create v0.1.0 dist/ltn-0.1.0-x86_64-pc-windows-msvc.zip dist/SHA256SUMS --title "ltn 0.1.0" --notes-file CHANGELOG.md

# map-mtl (user runs this, after replacing OWNER in Cargo.toml:8 and README.md:86)
cd D:\map-mtl-public
git remote add origin https://github.com/<OWNER>/map-mtl.git
git push -u origin main
```
