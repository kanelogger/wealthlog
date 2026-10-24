#!/usr/bin/env python3
"""Create a reviewed, history-free Wealthlog source snapshot under .local/."""

import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / ".local" / "public-release"
SNAPSHOT = OUTPUT / "source"
PREFIXES = ("apps/", "crates/", "packages/", "scripts/", "skills/", "tests/", "e2e/", "config/", ".cargo/")
ROOT_FILES = {
    ".gitignore", ".node-version", ".prettierignore", ".prettierrc.cjs",
    ".env.example", ".env.web.example", "Cargo.toml", "Cargo.lock",
    "LICENSE", "TRADEMARKS.md", "clippy.toml", "eslint.base.config.js",
    "eslint.config.js", "package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml",
    "rust-toolchain.toml", "tsconfig.base.json", "tsconfig.json", "tsconfig.node.json",
    "tsconfig.test.json", "vite.config.d.ts", "playwright.config.ts",
    "playwright.net-worth.config.ts", "playwright.profiles.config.ts",
}
PUBLIC_DOCS = {"docs/public-release/README.md": "README.md",
               "docs/public-release/customization.md": "docs/customization.md"}
SECRET_PATTERNS = {
    "private-key": re.compile(rb"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
    "github-token": re.compile(rb"(?:ghp_|github_pat_)[A-Za-z0-9_]{25,}"),
    "openai-token": re.compile(rb"sk-(?:proj-)?[A-Za-z0-9_-]{35,}"),
}
# Synthetic JWT signing fixture; keep only after its contents are reviewed.
KNOWN_TEST_FIXTURES = {"apps/server/tests/oidc_jwks_refresh.rs"}


def candidate_paths():
    raw = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=ROOT
    )
    return sorted(set(filter(None, raw.decode().split("\0"))))


def include(path):
    if path in PUBLIC_DOCS or path in ROOT_FILES:
        return True
    if not path.startswith(PREFIXES):
        return False
    if path.startswith(("apps/frontend/public/ticker-logos/", "apps/frontend/public/market-data/",
                        "apps/frontend/public/goals/", "apps/frontend/public/avatars/")):
        return False
    if path.endswith("/Entitlements.plist") or path == "apps/frontend/src/components/ticker-logo-filenames.test.ts":
        return False
    if Path(path).name.startswith(".env") and path not in {".env.example", ".env.web.example"}:
        return False
    return True


def main():
    if SNAPSHOT.exists():
        raise SystemExit(f"Refusing to replace existing snapshot: {SNAPSHOT}")
    OUTPUT.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix="source-", dir=OUTPUT))
    records = []
    failures = []
    try:
        for path in candidate_paths():
            if not include(path):
                continue
            source = ROOT / path
            if not source.exists():
                continue  # Removed tracked material is intentionally absent.
            if source.is_symlink():
                failures.append({"path": path, "reason": "symlink requires review"})
                continue
            if not source.is_file():
                continue
            dest_path = PUBLIC_DOCS.get(path, path)
            if path == "README.md" or path == "docs/customization.md":
                continue  # Public templates take their places.
            data = source.read_bytes()
            for category, pattern in SECRET_PATTERNS.items():
                if pattern.search(data) and path not in KNOWN_TEST_FIXTURES:
                    failures.append({"path": path, "reason": category})
            if b"/Users/" + b"kanehua/" in data:
                failures.append({"path": path, "reason": "personal absolute path"})
            dest = staging / dest_path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data)
            dest.chmod(source.stat().st_mode & 0o777)
            records.append({"path": dest_path, "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)})
            if path in PUBLIC_DOCS:
                template = staging / path
                template.parent.mkdir(parents=True, exist_ok=True)
                template.write_bytes(data)
                records.append({"path": path, "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)})
        if failures:
            raise RuntimeError("Snapshot checks failed: " + json.dumps(failures, ensure_ascii=False))
        if not (staging / "LICENSE").exists() or not (staging / "apps/frontend/public/legal/LICENSE.txt").exists():
            raise RuntimeError("License files missing")
        if (staging / "LICENSE").read_bytes() != (staging / "apps/frontend/public/legal/LICENSE.txt").read_bytes():
            raise RuntimeError("Repository and offline app license differ")
        if any((staging / p).exists() for p in ("apps/frontend/public/ticker-logos", "apps/frontend/public/market-data", "apps/frontend/public/avatars")):
            raise RuntimeError("Third-party logo directory remained in snapshot")
        staging.rename(SNAPSHOT)
        report = {"source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip(),
                  "source_worktree_has_changes": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
                  "files": sorted(records, key=lambda x: x["path"]),
                  "note": "No Git history included. Dependency license and asset audits remain release gates."}
        (OUTPUT / "manifest.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
        print(f"Created {SNAPSHOT} with {len(records)} files")
    except Exception:
        shutil.rmtree(staging)
        raise


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(exc, file=sys.stderr)
        sys.exit(1)
