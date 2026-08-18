#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""构建脚本：每次运行将版本号 +0.0.1（三处同步）后执行 tauri 构建。

用法：
    python build.py

行为：
    1. 读取当前版本号（以 src-tauri/tauri.conf.json 为准）
    2. 递增最后一段：0.1.0 -> 0.1.1
    3. 同步更新 package.json / tauri.conf.json / Cargo.toml 三处版本号
    4. 执行 npm run tauri build
    5. 构建失败时回滚版本号
"""
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
TAURI_CONF = ROOT / "src-tauri" / "tauri.conf.json"
PACKAGE_JSON = ROOT / "package.json"
CARGO_TOML = ROOT / "src-tauri" / "Cargo.toml"


def read_version() -> str:
    with open(TAURI_CONF, encoding="utf-8") as f:
        return json.load(f)["version"]


def bump_version(version: str) -> str:
    """递增最后一段版本号，保留 pre-release 后缀：0.1.0 -> 0.1.1"""
    m = re.match(r"^(\d+)\.(\d+)\.(\d+)(.*)$", version)
    if not m:
        raise ValueError(f"无法解析版本号: {version}")
    major, minor, patch, suffix = m.groups()
    return f"{major}.{minor}.{int(patch) + 1}{suffix}"


def write_json_version(path: Path, version: str) -> None:
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    data["version"] = version
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
        f.write("\n")


def update_version(version: str) -> None:
    write_json_version(PACKAGE_JSON, version)
    write_json_version(TAURI_CONF, version)

    text = CARGO_TOML.read_text(encoding="utf-8")
    new_text, n = re.subn(
        r'^version = "\d+\.\d+\.\d+[^"]*"',
        f'version = "{version}"',
        text,
        count=1,
        flags=re.M,
    )
    if n != 1:
        raise RuntimeError("Cargo.toml 中未找到 version 字段")
    CARGO_TOML.write_text(new_text, encoding="utf-8")


def build() -> int:
    print(">>> npm run tauri build")
    # Windows 下 npm 为 npm.cmd，shell=True 以保证可解析
    return subprocess.run("npm run tauri build", cwd=str(ROOT), shell=True).returncode


def main() -> None:
    old = read_version()
    new = bump_version(old)
    update_version(new)
    print(f"版本号: {old} -> {new}")

    code = build()
    if code != 0:
        print("构建失败，回滚版本号...")
        update_version(old)
        print(f"版本号已回滚: {new} -> {old}")
        sys.exit(code)

    print(f"构建成功，版本 {new}")
    print(f"安装包目录: {ROOT / 'src-tauri' / 'target' / 'release' / 'bundle'}")


if __name__ == "__main__":
    main()
