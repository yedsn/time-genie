#!/usr/bin/env python3

import argparse
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path


HTTP_TIMEOUT_SECS = 60
USER_AGENT = "time-genie-updater-json"


def fail(message: str) -> None:
    print(f"[updater-json] Error: {message}", file=sys.stderr, flush=True)
    raise SystemExit(1)


def log(message: str) -> None:
    print(message, flush=True)


def github_headers(token: str, *, accept: str = "application/vnd.github+json") -> dict[str, str]:
    return {
        "Accept": accept,
        "Authorization": f"Bearer {token}",
        "User-Agent": USER_AGENT,
        "X-GitHub-Api-Version": "2022-11-28",
    }


def request_json(method: str, url: str, *, token: str, data=None, accept: str = "application/vnd.github+json"):
    body = None
    headers = github_headers(token, accept=accept)
    if data is not None:
        body = json.dumps(data, ensure_ascii=False).encode("utf-8")
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=body, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=HTTP_TIMEOUT_SECS) as response:
            if response.status == 204:
                return None
            return json.load(response)
    except urllib.error.HTTPError as exc:
        body_text = exc.read().decode("utf-8", "ignore")
        fail(f"{method} {url} failed with HTTP {exc.code}: {body_text}")
    except urllib.error.URLError as exc:
        fail(f"{method} {url} failed: {exc}")


def download_text(url: str, *, token: str) -> str:
    req = urllib.request.Request(
        url,
        headers=github_headers(token, accept="application/octet-stream"),
    )
    try:
        with urllib.request.urlopen(req, timeout=HTTP_TIMEOUT_SECS) as response:
            return response.read().decode("utf-8").strip()
    except urllib.error.HTTPError as exc:
        body_text = exc.read().decode("utf-8", "ignore")
        fail(f"Download {url} failed with HTTP {exc.code}: {body_text}")
    except urllib.error.URLError as exc:
        fail(f"Download {url} failed: {exc}")


def upload_asset(upload_url_template: str, *, token: str, path: Path, name: str) -> None:
    upload_base = upload_url_template.split("{", 1)[0]
    upload_url = f"{upload_base}?{urllib.parse.urlencode({'name': name})}"
    headers = github_headers(token, accept="application/vnd.github+json")
    headers["Content-Type"] = "application/json"
    data = path.read_bytes()
    req = urllib.request.Request(upload_url, data=data, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=HTTP_TIMEOUT_SECS) as response:
            if response.status not in (200, 201):
                fail(f"Upload {name} returned HTTP {response.status}")
    except urllib.error.HTTPError as exc:
        body_text = exc.read().decode("utf-8", "ignore")
        fail(f"Upload {name} failed with HTTP {exc.code}: {body_text}")
    except urllib.error.URLError as exc:
        fail(f"Upload {name} failed: {exc}")


def asset_by_name(assets: list[dict]) -> dict[str, dict]:
    return {asset.get("name", ""): asset for asset in assets if asset.get("name")}


def find_asset(assets: list[dict], predicate, description: str) -> dict:
    matches = [asset for asset in assets if predicate(asset.get("name", ""))]
    if not matches:
        fail(f"Missing release asset: {description}")
    if len(matches) > 1:
        names = ", ".join(asset.get("name", "") for asset in matches)
        fail(f"Multiple release assets matched {description}: {names}")
    return matches[0]


def add_platform(platforms: dict, key: str, *, asset: dict, signature: str) -> None:
    platforms[key] = {
        "signature": signature,
        "url": asset["browser_download_url"],
    }


def signature_for(asset: dict, assets_by_name: dict[str, dict], *, token: str) -> str:
    sig_name = f"{asset['name']}.sig"
    sig_asset = assets_by_name.get(sig_name)
    if not sig_asset:
        fail(f"Missing signature asset: {sig_name}")
    return download_text(sig_asset["url"], token=token)


def build_latest_json(release: dict, *, token: str) -> dict:
    assets = release.get("assets") or []
    assets_by = asset_by_name(assets)

    windows_asset = find_asset(
        assets,
        lambda name: name.endswith("_x64-setup.exe"),
        "Windows x64 NSIS installer (*_x64-setup.exe)",
    )
    darwin_x64_asset = find_asset(
        assets,
        lambda name: name == "_x64.app.tar.gz" or name.endswith("_x64.app.tar.gz"),
        "macOS Intel app archive (*_x64.app.tar.gz)",
    )
    darwin_aarch64_asset = find_asset(
        assets,
        lambda name: name == "_aarch64.app.tar.gz" or name.endswith("_aarch64.app.tar.gz"),
        "macOS Apple Silicon app archive (*_aarch64.app.tar.gz)",
    )

    platforms: dict[str, dict] = {}
    windows_sig = signature_for(windows_asset, assets_by, token=token)
    x64_sig = signature_for(darwin_x64_asset, assets_by, token=token)
    aarch64_sig = signature_for(darwin_aarch64_asset, assets_by, token=token)

    add_platform(platforms, "windows-x86_64", asset=windows_asset, signature=windows_sig)
    add_platform(platforms, "windows-x86_64-nsis", asset=windows_asset, signature=windows_sig)
    add_platform(platforms, "darwin-x86_64", asset=darwin_x64_asset, signature=x64_sig)
    add_platform(platforms, "darwin-x86_64-app", asset=darwin_x64_asset, signature=x64_sig)
    add_platform(platforms, "darwin-aarch64", asset=darwin_aarch64_asset, signature=aarch64_sig)
    add_platform(platforms, "darwin-aarch64-app", asset=darwin_aarch64_asset, signature=aarch64_sig)

    version = release.get("tag_name", "").removeprefix("v")
    if not version:
        fail("Release tag_name is missing")

    return {
        "version": version,
        "notes": release.get("body") or "",
        "pub_date": datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z"),
        "platforms": platforms,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Generate and upload Tauri updater latest.json for a GitHub release.")
    parser.add_argument("--tag", required=True)
    parser.add_argument("--github-owner", required=True)
    parser.add_argument("--github-repo", required=True)
    parser.add_argument("--output", default="latest.json")
    return parser


def main() -> None:
    args = build_parser().parse_args()
    token = os.environ.get("GITHUB_TOKEN", "").strip() or os.environ.get("GH_TOKEN", "").strip()
    if not token:
        fail("Missing GITHUB_TOKEN or GH_TOKEN environment variable")

    release_url = f"https://api.github.com/repos/{args.github_owner}/{args.github_repo}/releases/tags/{args.tag}"
    release = request_json("GET", release_url, token=token)
    assets = release.get("assets") or []
    latest = build_latest_json(release, token=token)

    output = Path(args.output)
    output.write_text(json.dumps(latest, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    log(f"[updater-json] Wrote {output}")

    for asset in assets:
        if asset.get("name") == "latest.json":
            log("[updater-json] Deleting existing latest.json asset")
            request_json("DELETE", asset["url"], token=token)

    upload_asset(release["upload_url"], token=token, path=output, name="latest.json")
    log(f"[updater-json] Uploaded latest.json to {args.github_owner}/{args.github_repo} {args.tag}")


if __name__ == "__main__":
    main()
