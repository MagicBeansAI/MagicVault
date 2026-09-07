"""Read-only, coarse architecture-drift gate; not a semantic/security attestation."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sys

DOCUMENT = "docs/architecture.md"
BASELINE = "docs/architecture-baseline.json"


def digest(path):
    if path.is_symlink():
        raise ValueError("architecture inputs must not be symlinks")
    return hashlib.sha256(path.read_bytes()).hexdigest()


def package_identity(manifest):
    if manifest.is_symlink():
        raise ValueError("architecture manifests must not be symlinks")
    text = manifest.read_text(encoding="utf8")
    section = re.search(r"(?ms)^\[package\]\s*\n(.*?)(?=^\[|\Z)", text)
    if not section:
        raise ValueError("package manifest needs an explicit package section")
    result = []
    for key in ("name", "version"):
        match = re.search(r"(?m)^" + key + r"\s*=\s*[\"']([^\"'\n]+)[\"']\s*(?:#.*)?$", section[1])
        if not match:
            raise ValueError("review architecture version extraction after manifest format changes")
        result.append(match[1])
    return tuple(result)


def snapshot(root):
    root = root.resolve()
    if (root / "magicvault/Cargo.toml").is_file():
        project, primary = "MagicVault", "magicvault"
    elif (root / "tool-runtime-core/Cargo.toml").is_file():
        project, primary = "MagicRun", "tool-runtime-core"
    else:
        raise ValueError("unrecognized workspace")
    files = {"Cargo.toml": digest(root / "Cargo.toml")}
    versions = {}
    # These workspaces use top-level member directories. The root manifest is
    # fingerprinted too: moving/nesting members requires an explicit guard review.
    for manifest in sorted(root.glob("*/Cargo.toml")):
        if manifest.parent.name == "test-support":
            continue
        if manifest.parent.is_symlink():
            raise ValueError("architecture packages must not be symlinks")
        name, version = package_identity(manifest)
        versions[name] = version
        files[manifest.relative_to(root).as_posix()] = digest(manifest)
        for source in sorted((manifest.parent / "src").rglob("*")):
            if source.name == ".DS_Store":
                continue
            if source.is_symlink():
                raise ValueError("architecture sources must not be symlinks")
            if source.is_file():
                files[source.relative_to(root).as_posix()] = digest(source)
    if project == "MagicVault":
        files["Cargo.lock"] = digest(root / "Cargo.lock")
        extension = root / "extension"
        for source in sorted(extension.glob("*")):
            if source.name == ".DS_Store":
                continue
            if source.is_symlink():
                raise ValueError("extension sources must not be symlinks")
            if source.is_file():
                files[source.relative_to(root).as_posix()] = digest(source)
        versions["extension"] = json.loads((extension / "manifest.json").read_text(encoding="utf8"))["version"]
        # Packaging/installation and the release workflow are executable trust
        # boundaries too, not merely documentation around the Rust code.
        if (root / "npm").is_symlink():
            raise ValueError("distribution source directories must not be symlinks")
        distribution = list((root / "npm").glob("*.cjs"))
        if (root / "npm/README.md").exists():
            distribution.append(root / "npm/README.md")
        distribution += [root / name for name in (
            "scripts/package-npm.mjs", "scripts/qualify-package.mjs",
            "scripts/sign-release.sh", ".github/workflows/distribution.yml",
        ) if (root / name).exists()]
        for source in sorted(distribution):
            files[source.relative_to(root).as_posix()] = digest(source)
    version = versions[primary]
    document = root / DOCUMENT
    document_hash = digest(document)
    if f"Architecture version: `{version}`" not in document.read_text(encoding="utf8"):
        raise ValueError("architecture document version must match the primary package")
    return {
        "schema_version": 1, "project": project, "architecture_version": version,
        "document": DOCUMENT, "document_sha256": document_hash,
        "component_versions": dict(sorted(versions.items())),
        "source_sha256": dict(sorted(files.items())),
    }


def differences(expected, actual):
    result = []
    for key in ("schema_version", "project", "architecture_version", "document", "document_sha256", "component_versions"):
        if expected.get(key) != actual.get(key):
            result.append(key)
    before, after = expected.get("source_sha256", {}), actual["source_sha256"]
    if not isinstance(before, dict):
        return result + ["invalid source fingerprint map"]
    for name in sorted(set(before) | set(after)):
        if before.get(name) != after.get(name):
            result.append(name)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", action="store_true", help="print a candidate baseline; never writes or approves it")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    try:
        actual = snapshot(root)
        if args.snapshot:
            print(json.dumps(actual, indent=2) + "\n", end="")
            return 0
        expected = json.loads((root / BASELINE).read_text(encoding="utf8"))
        if not isinstance(expected, dict):
            raise ValueError("invalid architecture baseline")
        changed = differences(expected, actual)
        if changed:
            print("Architecture review required: " + ", ".join(changed), file=sys.stderr)
            print("Review docs/architecture.md before replacing the baseline; do not refresh it blindly.", file=sys.stderr)
            return 1
        print(f"{actual['project']} architecture {actual['architecture_version']}: baseline matches ({len(actual['source_sha256'])} source inputs)")
        return 0
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"Architecture check failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
