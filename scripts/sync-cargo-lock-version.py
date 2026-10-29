"""把 Cargo.lock 里 workspace 包的版本对齐到 Cargo.toml 的 [workspace.package] version。"""

import re
from pathlib import Path

import tomllib

ROOT = Path(__file__).resolve().parent.parent
PACKAGE_PREFIX = "voltnest-"

NAME_LINE = re.compile(r'^name = "(?P<name>[^"]+)"$')
VERSION_LINE = re.compile(r'^version = "(?P<version>[^"]*)"$')


def workspace_version() -> str:
    with (ROOT / "Cargo.toml").open("rb") as handle:
        return tomllib.load(handle)["workspace"]["package"]["version"]


def sync() -> list[str]:
    version = workspace_version()
    path = ROOT / "Cargo.lock"
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)

    current: str | None = None
    changed: list[str] = []
    for index, line in enumerate(lines):
        stripped = line.rstrip("\n")
        if stripped == "[[package]]":
            current = None
            continue
        name = NAME_LINE.match(stripped)
        if name:
            current = name.group("name")
            continue
        if not current or not current.startswith(PACKAGE_PREFIX):
            continue
        match = VERSION_LINE.match(stripped)
        if match and match.group("version") != version:
            lines[index] = f'version = "{version}"\n'
            changed.append(f"{current}: {match.group('version')} -> {version}")

    if changed:
        path.write_text("".join(lines), encoding="utf-8")
    return changed


def main() -> int:
    changed = sync()
    for item in changed:
        print(item)
    print(f"{len(changed)} package(s) updated")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
