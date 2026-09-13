# LTN.ENG — Troubleshooting & Diagnostics Guide

## 1. Common Diagnostics & Fix Procedures

### Issue 1: `ltn doctor` reports `BLOCKED_UNAUTHENTICATED` for cloud providers
- **Cause**: Cloud provider environment variables (e.g. `AWS_ACCESS_KEY_ID`, `GOOGLE_APPLICATION_CREDENTIALS`, `AZURE_SUBSCRIPTION_ID`) are missing.
- **Fix**: Set environment variables in your terminal or configure `D:\.ltn-graph\config.toml` to use `default_provider = "LOCAL_DOCKER"`.
```bash
ltn config --set-provider LOCAL_DOCKER
```

---

### Issue 2: `Z3 SMT Solver Unsatisfiable` Error
- **Cause**: Project resource demands (e.g. 128 CPU cores or 100GB VRAM) exceed the available cluster capacity.
- **Fix**: Inspect the extracted **Minimal Unsatisfiable Core (MUS)** in `ltn explain` or run `ltn architect` to generate a `HIGH-PERFORMANCE` cloud cluster model.
```bash
ltn explain
ltn architect ./my-app
```

---

### Issue 3: Container Sandbox Not Responding
- **Cause**: Docker service in WSL or container stopped after host reboot.
- **Fix**: Run `install-ltn.ps1` or start the container sandbox via WSL:
```bash
wsl -d Ubuntu-24.04 bash -c "sudo service docker start && sudo docker start ltn-sandbox"
```

---

### Issue 4: Rolling Back an Unintended State Mutation
- **Cause**: Deployment or configuration mutation needs to be undone.
- **Fix**: Run `ltn rollback` to revert state machine snapshot:
```bash
ltn rollback
```

---

## 2. Verification Health Check Commands

```bash
# 1. Run diagnostic checks
ltn doctor

# 2. Query operational status
ltn status

# 3. Check Z3 solver proof
ltn explain

# 4. View active settings
ltn config
```
