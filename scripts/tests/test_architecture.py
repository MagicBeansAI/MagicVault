"""Synthetic architecture-drift cases; never compile or execute product code."""
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("architecture_gate", Path(__file__).resolve().parents[1] / "check_architecture.py")
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)


class Architecture(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="architecture-fixture-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.write("Cargo.toml", '[workspace]\nmembers = ["tool-runtime-core"]\n')
        self.write("tool-runtime-core/Cargo.toml", '[package]\nname = "tool-runtime-core"\nversion = "0.1.73"\n')
        self.write("tool-runtime-core/src/lib.rs", "pub fn fixture() {}\n")
        self.write("docs/architecture.md", "# Architecture\nArchitecture version: `0.1.73`\n")
        self.before = GATE.snapshot(self.root)

    def write(self, name, content):
        file = self.root / name
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(content, encoding="utf8")

    def changed(self):
        return GATE.differences(self.before, GATE.snapshot(self.root))

    def test_unchanged_snapshot_is_deterministic_and_read_only(self):
        self.assertEqual(self.changed(), [])
        self.assertFalse((self.root / GATE.BASELINE).exists())

    def test_source_change_is_detected(self):
        self.write("tool-runtime-core/src/lib.rs", "pub fn changed() {}\n")
        self.assertIn("tool-runtime-core/src/lib.rs", self.changed())

    def test_added_source_is_detected(self):
        self.write("tool-runtime-core/src/new.rs", "pub fn new() {}\n")
        self.assertIn("tool-runtime-core/src/new.rs", self.changed())

    def test_finder_metadata_is_not_an_architecture_input(self):
        self.write("tool-runtime-core/src/.DS_Store", "synthetic Finder metadata")
        self.assertEqual(self.changed(), [])

    def test_removed_source_is_detected(self):
        (self.root / "tool-runtime-core/src/lib.rs").unlink()
        self.assertIn("tool-runtime-core/src/lib.rs", self.changed())

    def test_manifest_dependency_change_is_detected(self):
        self.write("Cargo.toml", '[workspace]\nmembers = ["tool-runtime-core", "new"]\n')
        self.assertIn("Cargo.toml", self.changed())

    def test_document_change_requires_baseline_review(self):
        self.write("docs/architecture.md", "Architecture version: `0.1.73`\nChanged boundary\n")
        self.assertIn("document_sha256", self.changed())

    def test_version_drift_requires_document_update(self):
        self.write("tool-runtime-core/Cargo.toml", '[package]\nname = "tool-runtime-core"\nversion = "0.2.0"\n')
        with self.assertRaises(ValueError):
            GATE.snapshot(self.root)

    def test_version_change_still_invalidates_old_baseline_after_document_update(self):
        self.write("tool-runtime-core/Cargo.toml", '[package]\nname = "tool-runtime-core"\nversion = "0.2.0"\n')
        self.write("docs/architecture.md", "Architecture version: `0.2.0`\n")
        self.assertIn("component_versions", self.changed())

    def test_source_symlink_is_refused(self):
        (self.root / "tool-runtime-core/src/link.rs").symlink_to(self.root / "Cargo.toml")
        with self.assertRaises(ValueError):
            GATE.snapshot(self.root)

    def test_vault_extension_and_lockfile_are_covered(self):
        self.write("magicvault/Cargo.toml", '[package]\nname = "magicvault"\nversion = "0.3.0"\n')
        self.write("magicvault/src/main.rs", "fn main() {}\n")
        self.write("Cargo.lock", "# synthetic lock\n")
        self.write("extension/manifest.json", '{"version":"0.3.0"}\n')
        self.write("extension/worker.js", "// synthetic worker\n")
        self.write("docs/architecture.md", "Architecture version: `0.3.0`\n")
        before = GATE.snapshot(self.root)
        self.write("extension/worker.js", "// changed worker\n")
        self.write("Cargo.lock", "# changed lock\n")
        changed = GATE.differences(before, GATE.snapshot(self.root))
        self.assertIn("extension/worker.js", changed)
        self.assertIn("Cargo.lock", changed)

    def test_distribution_launchers_scripts_and_workflow_are_covered(self):
        self.write("magicvault/Cargo.toml", '[package]\nname = "magicvault"\nversion = "0.5.0"\n')
        self.write("Cargo.lock", "# synthetic\n")
        self.write("extension/manifest.json", '{"version":"0.3.0"}\n')
        self.write("docs/architecture.md", "Architecture version: `0.5.0`\n")
        before = GATE.snapshot(self.root)
        for name in ["npm/launcher.cjs", "scripts/package-npm.mjs", "scripts/sign-release.sh", ".github/workflows/distribution.yml", ".github/workflows/qualification.yml"]:
            self.write(name, "synthetic distribution input\n")
            self.assertIn(name, GATE.differences(before, GATE.snapshot(self.root)))
        (self.root / "npm/launcher.cjs").unlink()
        (self.root / "npm/launcher.cjs").symlink_to(self.root / "Cargo.lock")
        with self.assertRaises(ValueError):
            GATE.snapshot(self.root)


if __name__ == "__main__":
    unittest.main()
