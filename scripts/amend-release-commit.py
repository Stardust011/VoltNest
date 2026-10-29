"""用 Git Data API 把工作区文件 amend 到分支最新提交，生成 GitHub 服务端签名的提交。"""

import base64
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path

API = os.environ.get("GITHUB_API_URL", "https://api.github.com").rstrip("/")
REPO = os.environ["GITHUB_REPOSITORY"]
TOKEN = os.environ["GITHUB_TOKEN"]


def request(method: str, path: str, payload: dict | None = None) -> dict:
    body = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(
        f"{API}/repos/{REPO}{path}",
        data=body,
        method=method,
        headers={
            "Authorization": f"Bearer {TOKEN}",
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
        raise SystemExit("用法: amend-release-commit.py <branch> <path>...")

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
        print("文件无变化，跳过")
        return 0

    # 不传 author / committer，由 GitHub 按 token 身份填充并签名
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
