#!/usr/bin/env python3
"""Scan deliverable files for test credentials and self-test the scanner.

Secrets are held in memory and never printed. Output contains only labels,
relative paths and counts. The scan covers every regular file under each
`--root`; only explicitly registered credential files may be exempted.
"""

import argparse
import json
import os
import stat
import sys
import tempfile
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Set

MAX_BYTES = 8 * 1024 * 1024


class ScanError(Exception):
    pass


def text_fields(value):
    if isinstance(value, str):
        yield value
        try:
            nested = json.loads(value)
        except (ValueError, RecursionError):
            return
        yield from text_fields(nested)
    elif isinstance(value, dict):
        for key, item in value.items():
            yield str(key)
            yield from text_fields(item)
    elif isinstance(value, list):
        for item in value:
            yield from text_fields(item)


def read_secret(path: Path) -> str:
    info = os.lstat(path)
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISREG(info.st_mode):
        raise ScanError("secret file must be a regular file: %s" % path.name)
    if stat.S_IMODE(info.st_mode) not in (0o400, 0o600):
        raise ScanError("secret file must be 0400/0600: %s" % path.name)
    value = path.read_text(encoding="utf-8").strip()
    if len(value) < 8:
        raise ScanError("secret is too short to scan meaningfully: %s" % path.name)
    return value


def files_under(roots: Iterable[str], allow: Set[str]) -> Iterable[Path]:
    seen: Set[str] = set()

    def walk_error(error: OSError) -> None:
        raise ScanError("cannot enumerate scan root: %s" % error.filename)

    for root in roots:
        path = Path(root)
        info = path.lstat()
        if stat.S_ISLNK(info.st_mode):
            raise ScanError("scan root must not be a symlink: %s" % path)
        if stat.S_ISREG(info.st_mode):
            candidates = [path]
        elif stat.S_ISDIR(info.st_mode):
            candidates = []
            for directory, subdirs, filenames in os.walk(path, onerror=walk_error):
                for name in subdirs:
                    child = Path(directory) / name
                    if child.is_symlink():
                        raise ScanError("scan directory must not be a symlink: %s" % child)
                candidates.extend(Path(directory) / name for name in filenames)
        else:
            raise ScanError("unsupported scan root: %s" % path)
        for candidate in candidates:
            info = candidate.lstat()
            if not stat.S_ISREG(info.st_mode):
                raise ScanError("scan entry must be a regular file: %s" % candidate)
            text = str(candidate.resolve())
            if text in seen or text in allow:
                continue
            seen.add(text)
            yield candidate


def scan(
    roots: List[str],
    secrets: Dict[str, str],
    allow: Set[str],
) -> Dict[str, object]:
    hits: List[Dict[str, object]] = []
    scanned = 0
    skipped = 0
    for path in files_under(roots, allow):
        try:
            info = path.stat()
        except OSError:
            skipped += 1
            continue
        if info.st_size > MAX_BYTES:
            skipped += 1
            continue
        try:
            data = path.read_bytes()
        except OSError:
            skipped += 1
            continue
        scanned += 1
        decoded = []
        try:
            decoded.extend(text_fields(json.loads(data)))
        except (ValueError, UnicodeError, RecursionError):
            # Also inspect independently encoded JSONL records.
            for line in data.splitlines():
                try:
                    decoded.extend(text_fields(json.loads(line)))
                except (ValueError, UnicodeError, RecursionError):
                    pass
        for label, secret in secrets.items():
            needle = secret.encode("utf-8")
            count = data.count(needle)
            parsed_count = sum(text.count(secret) for text in decoded)
            count = max(count, parsed_count)
            if count:
                try:
                    display = str(path.resolve())
                except OSError:
                    display = str(path)
                hits.append({"label": label, "path": display, "count": count})
    return {
        "ok": not hits and skipped == 0 and scanned > 0 and bool(secrets),
        "roots": [str(Path(root).resolve()) for root in roots],
        "scanned": scanned,
        "skipped": skipped,
        "labels": sorted(secrets),
        "hits": hits,
    }


def self_test() -> Dict[str, object]:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        positive = root / "positive.txt"
        negative = root / "negative.txt"
        secret = "wfp_self_test_0123456789"
        positive.write_text("prefix " + secret + " suffix", encoding="utf-8")
        negative.write_text("no secret here", encoding="utf-8")
        result = scan([str(root)], {"self-test": secret}, set())
        detected = any(item["label"] == "self-test" for item in result["hits"])
        clean = scan([str(root)], {"other": "wfp_other_secret_9876543210"}, set())
        return {
            "ok": detected and clean["ok"],
            "positiveDetected": detected,
            "negativeClean": clean["ok"],
        }


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", action="append", default=[])
    parser.add_argument("--secret", action="append", default=[], metavar="LABEL=PATH")
    parser.add_argument("--allow-file", action="append", default=[])
    parser.add_argument("--out")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    try:
        if args.self_test:
            result = self_test()
            print(json.dumps(result, ensure_ascii=False, sort_keys=True))
            return 0 if result["ok"] else 1
        if not args.root:
            raise ScanError("pass at least one --root")
        secrets: Dict[str, str] = {}
        credential_paths: Set[str] = set()
        for entry in args.secret:
            if "=" not in entry:
                raise ScanError("--secret must be LABEL=PATH")
            label, raw_path = entry.split("=", 1)
            secrets[label] = read_secret(Path(raw_path))
            credential_paths.add(str(Path(raw_path).resolve()))
        allow = {str(Path(item).resolve()) for item in args.allow_file}
        if not allow.issubset(credential_paths):
            raise ScanError("only registered credential files may be exempted")
        result = scan(args.root, secrets, allow)
        if args.out:
            Path(args.out).write_text(
                json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
            )
        print(json.dumps(result, ensure_ascii=False, sort_keys=True))
        return 0 if result["ok"] else 1
    except (ScanError, OSError) as error:
        print(
            json.dumps(
                {"ok": False, "error": {"code": "scan", "message": str(error)}},
                ensure_ascii=False,
            )
        )
        return 2


if __name__ == "__main__":
    sys.exit(main())
