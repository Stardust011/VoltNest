"""Amend files into the branch head commit through the GitHub Git Data API, so GitHub signs it."""

import base64
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path

API = os.environ.get("GITHUB_API_URL", "https://api.github.com").rstrip("/")


def env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise SystemExit(f"missing environment variable: {name}")
    return value


def request(method: str, path: str, payload: dict | None = None) -> dict:
    body = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(
        f"{API}/repos/{env('GITHUB_REPOSITORY')}{path}",
        data=body,
        method=method,
        headers={
            "Authorization": f"Bearer {env('GITHUB_TOKEN')}",
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as error:
        raise SystemExit(
            f"{method} {path} -> {error.code} {error.read().decode(errors='replace')}"
        ) from error


def create_blob(path: str) -> str:
    content = base64.b64encode(Path(path).read_bytes()).decode()
    return request("POST", "/git/blobs", {"content": content, "encoding": "base64"})["sha"]


def main() -> int:
    if len(sys.argv) < 3:
        raise SystemExit("usage: amend-release-commit.py <branch> <path>...")

    branch = sys.argv[1]
    head = request("GET", f"/git/ref/heads/{branch}")["object"]["sha"]
    commit = request("GET", f"/git/commits/{head}")

    entries = [
        {
            "path": Path(item).as_posix(),
            "mode": "100644",
            "type": "blob",
            "sha": create_blob(item),
        }
        for item in sys.argv[2:]
    ]
    tree = request("POST", "/git/trees", {"base_tree": commit["tree"]["sha"], "tree": entries})
    if tree["sha"] == commit["tree"]["sha"]:
        print(f"{branch}: no content change, skipping")
        return 0

    # No author/committer supplied: GitHub fills them from the token and signs the commit.
    new_commit = request(
        "POST",
        "/git/commits",
        {
            "message": commit["message"],
            "tree": tree["sha"],
            "parents": [parent["sha"] for parent in commit["parents"]],
        },
    )
    request("PATCH", f"/git/refs/heads/{branch}", {"sha": new_commit["sha"], "force": True})
    print(f"{branch}: {head[:7]} -> {new_commit['sha'][:7]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
