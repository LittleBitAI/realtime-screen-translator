import tempfile
import unittest
from contextlib import chdir
from pathlib import Path

from certify import RECEIPT, REPORT_FILES, certify, fingerprint, git


class ReceiptBinding(unittest.TestCase):
    def test_only_the_recorded_inputs_can_receive_the_observations(self):
        with tempfile.TemporaryDirectory() as folder, chdir(folder):
            git("init", "-q")
            git("config", "user.name", "Fixture")
            git("config", "user.email", "fixture@example.invalid")
            files = {
                "src/page.js": "Runtime inputs",
                "src-tauri/src/library.rs": "Runtime inputs",
                "src-tauri/tests/library.rs": "Runtime inputs",
                "Cargo.lock": "Runtime inputs",
                ".wiki/adapter.toml": "Runtime inputs",
                ".wiki/corpus.json": None,
                ".wiki/graph.json": None,
                "e2e/receipts/saved-lists/android.png": "Evidence inputs",
                ".github/workflows/ci.yml": "Evidence inputs",
            }
            for name in files:
                path = Path(name)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"fixture")
            git("add", ".")
            git("commit", "-qm", "Test inputs")
            Path(RECEIPT).write_text(
                f"Runtime inputs SHA-256: {fingerprint('HEAD', REPORT_FILES)}\n"
                f"Evidence inputs SHA-256: {fingerprint('HEAD', [RECEIPT])}\n",
                encoding="utf-8",
            )
            git("add", RECEIPT)
            git("commit", "-qm", "Record observations")
            head = git("rev-parse", "HEAD").decode().strip()
            self.assertIn(f"Certified checkout: {head}\nreceipt_matches_reviewed_head: PASS", certify(head))

            for name, expected_error in files.items():
                with self.subTest(name=name):
                    Path(name).write_bytes(b"changed fixture")
                    git("add", name)
                    tree = git("write-tree").decode().strip()
                    changed = git("commit-tree", tree, "-m", "Changed input").decode().strip()
                    Path(name).write_bytes(b"fixture")
                    git("add", name)
                    if expected_error is None:
                        self.assertIn(
                            f"Certified checkout: {changed}\nreceipt_matches_reviewed_head: PASS",
                            certify(changed),
                        )
                    else:
                        with self.assertRaisesRegex(ValueError, expected_error):
                            certify(changed)


if __name__ == "__main__":
    unittest.main()
