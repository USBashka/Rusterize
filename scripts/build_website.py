"""Build the Rust website and its static browser shell for any subdirectory."""
from pathlib import Path
import json
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]

def main():
    subprocess.run([sys.executable, str(ROOT / "scripts/build.py"), "--platform", "web", "--package", "rusterize-website"], cwd=ROOT, check=True)
    out = ROOT / "dist/web/rusterize-website"
    shutil.copytree(ROOT / "examples/website/web", out, dirs_exist_ok=True)
    (out / ".nojekyll").write_text("", encoding="utf-8")
    sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    (out / "version.json").write_text(json.dumps({"commit": sha, "package": "rusterize-website"}) + "\n", encoding="utf-8")
    print(f"Website: {out}")

if __name__ == "__main__":
    main()
