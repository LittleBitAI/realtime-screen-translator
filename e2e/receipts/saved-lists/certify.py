"""Emit the recorded observations with the identity of the checkout they cover."""

import hashlib
import os
import subprocess
import sys
from fnmatch import fnmatchcase
from pathlib import Path


RECEIPT = "e2e/receipts/saved-lists/verification.txt"
REPORT_FILES = ("e2e/receipts/saved-lists/**", ".github/workflows/ci.yml")
WIKI_INDEXES = (".wiki/corpus.json", ".wiki/graph.json")


def git(*args):
    return subprocess.check_output(["git", *args])


def fingerprint(revision, exclusions):
    digest = hashlib.sha256()
    exclusions = (*WIKI_INDEXES, *exclusions)
    for entry in git("ls-tree", "-r", "-z", revision).split(b"\0"):
        if entry:
            path = entry.split(b"\t", 1)[1]
            if not any(fnmatchcase(path, pattern.encode()) for pattern in exclusions):
                digest.update(entry + b"\0")
    return digest.hexdigest()


def certify(revision):
    head = git("rev-parse", f"{revision}^{{commit}}").decode().strip()
    receipt = git("show", f"{head}:{RECEIPT}").decode("utf-8")
    for label, excluded in [("Runtime inputs", REPORT_FILES), ("Evidence inputs", [RECEIPT])]:
        digest = fingerprint(head, excluded)
        if f"{label} SHA-256: {digest}" not in receipt.splitlines():
            raise ValueError(f"{label} differ from the recorded inputs for {head}")
    return f"Certified checkout: {head}\nreceipt_matches_reviewed_head: PASS\n{receipt}"


if __name__ == "__main__":
    certificate = certify("HEAD")
    if os.environ.get("GITHUB_ACTIONS") == "true":
        head = git("rev-parse", "HEAD").decode().strip()
        if head != os.environ["GITHUB_SHA"]:
            raise ValueError("Push and checkout identify different commits")
        run = f"{os.environ['GITHUB_SERVER_URL']}/{os.environ['GITHUB_REPOSITORY']}/actions/runs/{os.environ['GITHUB_RUN_ID']}"
        certificate = f"Hosted Gate step: PASS on {head}\nRun: {run}\n{certificate}"
        Path("artifacts/saved-list-verification.txt").write_text(certificate, encoding="utf-8")
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as summary:
            summary.write(f"\n```text\n{certificate}\n```\n")
    sys.stdout.buffer.write(certificate.encode("utf-8"))
