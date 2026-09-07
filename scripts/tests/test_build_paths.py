"""Build-routing fixtures only. Cargo is replaced by a harmless recorder."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
PROJECT = "magicvault" if (ROOT / "magicvault-core").is_dir() else "magicrun"


class BuildPaths(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="build-routing-")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.volume = self.base / "external disk"
        self.volume.mkdir()
        self.environment = os.environ.copy()
        for name in ("CARGO_TARGET_DIR", "BUILD_VOLUME", "MAKEFLAGS", "MFLAGS", "MAKEOVERRIDES", "GNUMAKEFLAGS", "MAKEFILES"):
            self.environment.pop(name, None)

    def make(self, *arguments, environment=None):
        return subprocess.run(
            ["make", "--no-print-directory", "-s", f"BUILD_VOLUME={self.volume}", *arguments],
            cwd=ROOT, env=environment or self.environment, text=True,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True, timeout=20,
        ).stdout.strip()

    def selected(self):
        return self.make("print-target-dir")

    def test_external_volume_is_selected_without_creating_cache(self):
        expected = self.volume / PROJECT / "builds"
        self.assertEqual(self.selected(), str(expected))
        self.assertFalse(expected.exists())

    def test_missing_volume_falls_back_without_creating_mount(self):
        self.volume.rmdir()
        self.assertEqual(self.selected(), str(ROOT / "target"))
        self.assertFalse(self.volume.exists())

    def test_existing_writable_cache_and_trailing_volume_slashes(self):
        expected = self.volume / PROJECT / "builds"
        expected.mkdir(parents=True)
        self.assertEqual(self.make("print-target-dir", f"BUILD_VOLUME={self.volume}///"), str(expected))

    def test_root_is_not_an_automatic_build_volume(self):
        self.assertEqual(self.make("print-target-dir", "BUILD_VOLUME=/"), str(ROOT / "target"))

    def test_volume_file_is_not_a_directory(self):
        self.volume.rmdir()
        self.volume.touch()
        self.assertEqual(self.selected(), str(ROOT / "target"))

    def test_existing_file_blocks_preferred_cache(self):
        (self.volume / PROJECT).touch()
        self.assertEqual(self.selected(), str(ROOT / "target"))

    def test_symlink_cache_is_not_adopted(self):
        (self.volume / PROJECT).symlink_to(self.base, target_is_directory=True)
        self.assertEqual(self.selected(), str(ROOT / "target"))

    @unittest.skipIf(hasattr(os, "geteuid") and os.geteuid() == 0, "root bypasses fixture permission bits")
    def test_unwritable_cache_falls_back(self):
        cache = self.volume / PROJECT / "builds"
        cache.mkdir(parents=True)
        cache.chmod(0o500)
        try:
            self.assertEqual(self.selected(), str(ROOT / "target"))
        finally:
            cache.chmod(0o700)

    def test_environment_override_is_preserved(self):
        value = str(self.base / "explicit target")
        environment = dict(self.environment, CARGO_TARGET_DIR=value)
        self.assertEqual(self.make("print-target-dir", environment=environment), value)

    def test_command_line_override_wins_over_environment(self):
        value = str(self.base / "command-line target")
        environment = dict(self.environment, CARGO_TARGET_DIR=str(self.base / "environment"))
        self.assertEqual(self.make("print-target-dir", f"CARGO_TARGET_DIR={value}", environment=environment), value)

    def test_empty_explicit_target_is_an_error_not_silent_fallback(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.make("print-target-dir", "CARGO_TARGET_DIR=")

    def test_cargo_recipes_receive_exported_target(self):
        executable = self.base / "bin"
        executable.mkdir()
        cargo = executable / "cargo"
        cargo.write_text('#!/bin/sh\nprintf "RECORDED_CARGO_TARGET=%s\\n" "$CARGO_TARGET_DIR"\n', encoding="utf8")
        cargo.chmod(0o700)
        environment = dict(self.environment, PATH=str(executable) + os.pathsep + self.environment.get("PATH", ""))
        targets = (["check", "test", "build-standalone", "test-browser-native", "test-cli-native", "test-public-web", "sync-lockfile"]
                   if PROJECT == "magicvault" else ["check", "build", "test", "test-lifecycle", "inventory", "classification", "replay"])
        expected = "RECORDED_CARGO_TARGET=" + str(self.volume / PROJECT / "builds")
        for target in targets:
            with self.subTest(target=target):
                output = self.make(target, environment=environment)
                self.assertIn(expected, output.splitlines())


if __name__ == "__main__":
    unittest.main()
