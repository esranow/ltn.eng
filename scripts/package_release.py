"""Build the Windows release archive of ltn from target/release.

    python scripts/package_release.py [Z3_DIR]

Writes dist/ltn-<version>-x86_64-pc-windows-msvc.zip and a SHA256SUMS file.
Run `cargo build --release -p ltn-cli --bin ltn` first.
"""

import hashlib
import os
import re
import sys
import zipfile

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
Z3 = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("Z3_DIR", r"D:\beam\z3-bin\z3-4.12.2-x64-win")

version = re.search(r'^version\s*=\s*"([^"]+)"', open(os.path.join(REPO, "crates", "ltn-cli", "Cargo.toml"), encoding="utf-8").read(), re.M).group(1)
name = f"ltn-{version}-x86_64-pc-windows-msvc"
files = {
    "ltn.exe": os.path.join(REPO, "target", "release", "ltn.exe"),
    "libz3.dll": os.path.join(Z3, "bin", "libz3.dll"),
    "THIRD_PARTY/z3-LICENSE.txt": os.path.join(Z3, "LICENSE.txt"),
    "README.md": os.path.join(REPO, "README.md"),
    "CHANGELOG.md": os.path.join(REPO, "CHANGELOG.md"),
    "examples/claude_mcp.json": os.path.join(REPO, "examples", "claude_mcp.json"),
    "examples/adk_agent.py": os.path.join(REPO, "examples", "adk_agent.py"),
}
missing = [p for p in files.values() if not os.path.isfile(p)]
if missing:
    sys.exit(f"missing: {missing}")
dist = os.path.join(REPO, "dist")
os.makedirs(dist, exist_ok=True)
archive = os.path.join(dist, name + ".zip")
with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
    for arc, src in files.items():
        z.write(src, f"{name}/{arc}")
digest = hashlib.sha256(open(archive, "rb").read()).hexdigest()
with open(os.path.join(dist, "SHA256SUMS"), "w", newline="\n") as f:
    f.write(f"{digest}  {name}.zip\n")
print(f"{archive}\n{digest}")
