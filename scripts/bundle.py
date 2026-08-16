import argparse
import os
import platform
import shutil
import subprocess
import sys
import tomllib
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence

APP_NAME = "MinnowSnap"
APP_PACKAGE = "minnow-app"
APP_MANIFEST = Path("crates") / APP_PACKAGE / "Cargo.toml"
PROJECT_ROOT = Path(__file__).resolve().parent.parent
SCRIPT_DIR = Path(__file__).resolve().parent
DEPLOY_DIR = PROJECT_ROOT / "target" / "deploy"
WINDOWS_DEBUG_GLOBS = ("*.pdb", "*.ilk", "*.exp", "*.lib", "*.a", "*.cmake")


@dataclass(frozen=True)
class DistOptions:
    upx: bool
    upx_aggressive: bool
    cargo_profile: str
    zip_lzma: bool


def print_action(action: str, message: str) -> None:
    print(f"{action:>12} {message}")


def fail(message: str) -> None:
    print(message)
    raise SystemExit(1)


def run_command(cmd: Sequence[str], *, cwd: Path = PROJECT_ROOT, silent: bool = False) -> None:
    cmd_list = list(cmd)
    run_kwargs: dict[str, object] = {"cwd": cwd, "check": True}
    if silent:
        run_kwargs["stdout"] = subprocess.PIPE
        run_kwargs["stderr"] = subprocess.PIPE
    try:
        subprocess.run(cmd_list, **run_kwargs)
    except subprocess.CalledProcessError as exc:
        if silent:
            if exc.stdout:
                sys.stdout.buffer.write(exc.stdout)
            if exc.stderr:
                sys.stderr.buffer.write(exc.stderr)
        fail(f"\nError executing command: {' '.join(cmd_list)}")


def get_bool_env(name: str, default: bool) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


def build_options(args: argparse.Namespace) -> DistOptions:
    profile = (os.getenv("MINNOWSNAP_CARGO_PROFILE") or "release").strip()
    return DistOptions(
        upx=args.upx or get_bool_env("MINNOWSNAP_USE_UPX", False),
        upx_aggressive=args.upx_aggressive or get_bool_env("MINNOWSNAP_UPX_AGGRESSIVE", False),
        cargo_profile=profile,
        zip_lzma=get_bool_env("MINNOWSNAP_ZIP_LZMA", False),
    )


def get_version() -> str:
    cargo_toml = PROJECT_ROOT / APP_MANIFEST
    with cargo_toml.open("rb") as f:
        data = tomllib.load(f)
    return str(data.get("package", {}).get("version", "0.0.0"))


def get_arch() -> str:
    machine = platform.machine().lower()
    if machine in {"x86_64", "amd64"}:
        return "x86_64"
    if machine in {"arm64", "aarch64"}:
        return "aarch64"
    return machine


def get_system_name() -> str:
    return platform.system().lower().replace("darwin", "macos")


def get_target_dir(profile: str) -> Path:
    return PROJECT_ROOT / "target" / profile


def ensure_clean_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True, exist_ok=True)


def clean_deploy_dir(name: str = APP_NAME) -> tuple[Path, Path]:
    ensure_clean_dir(DEPLOY_DIR)
    bundle_dir = DEPLOY_DIR / name
    bundle_dir.mkdir(parents=True, exist_ok=True)
    return DEPLOY_DIR, bundle_dir


def get_dir_size(path: Path) -> int:
    total = 0
    for entry in path.rglob("*"):
        if entry.is_file():
            try:
                total += entry.stat().st_size
            except OSError:
                continue
    return total


def format_size(size_bytes: int) -> str:
    return f"{size_bytes / (1024 * 1024):.2f} MB"


def top_files(path: Path, limit: int = 15) -> list[Path]:
    if not path.exists():
        return []
    files = [p for p in path.rglob("*") if p.is_file()]
    files.sort(key=lambda p: p.stat().st_size, reverse=True)
    return files[:limit]


def create_zip(src_dir: Path, zip_path: Path, *, use_lzma: bool) -> None:
    compression = zipfile.ZIP_LZMA if use_lzma else zipfile.ZIP_DEFLATED
    zip_kwargs = {"compression": compression}
    if not use_lzma:
        zip_kwargs["compresslevel"] = 9

    with zipfile.ZipFile(zip_path, "w", **zip_kwargs) as zf:
        for root, _, files in os.walk(src_dir):
            for file in files:
                file_path = Path(root) / file
                try:
                    arcname = file_path.relative_to(DEPLOY_DIR)
                    zf.write(file_path, arcname)
                except ValueError:
                    zf.write(file_path, file_path.name)


def maybe_compress_exe_with_upx(exe_path: Path, *, enabled: bool, aggressive: bool) -> None:
    if not enabled:
        return
    upx = shutil.which("upx")
    if not upx:
        print_action("Warning", "UPX enabled but upx not found; skip executable compression")
        return

    backup_path = exe_path.with_suffix(exe_path.suffix + ".unpacked")
    shutil.copy2(exe_path, backup_path)

    if aggressive:
        cmd = [upx, "--best", "--lzma", str(exe_path)]
        mode = "aggressive"
    else:
        cmd = [
            upx,
            "-1",
            "--lzma",
            "--compress-exports=0",
            "--compress-icons=0",
            "--compress-resources=0",
            "--strip-relocs=0",
            str(exe_path),
        ]
        mode = "safe"

    print_action("Compressing", f"executable with upx ({mode} mode)")

    try:
        subprocess.run(cmd, cwd=PROJECT_ROOT, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        subprocess.run([upx, "-t", str(exe_path)], cwd=PROJECT_ROOT, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    except subprocess.CalledProcessError as exc:
        sys.stdout.buffer.write(exc.stdout or b"")
        sys.stderr.buffer.write(exc.stderr or b"")
        shutil.copy2(backup_path, exe_path)
        print_action("Warning", "UPX failed or test failed; restored original executable")
    finally:
        if not get_bool_env("MINNOWSNAP_KEEP_UPX_BACKUP", False):
            backup_path.unlink(missing_ok=True)


def print_bundle_report(bundle_dir: Path, *, max_files: int = 12) -> None:
    print_action("Report", "largest files in bundle")
    for path in top_files(bundle_dir, limit=max_files):
        rel = path.relative_to(bundle_dir).as_posix()
        print(f"            {format_size(path.stat().st_size):>10}  {rel}")


def cargo_build(profile: str) -> None:
    print_action("Building", f"profile={profile} (cargo)")
    if profile == "release":
        run_command(["cargo", "build", "-p", APP_PACKAGE, "--bin", APP_NAME, "--release"])
    else:
        run_command(["cargo", "build", "-p", APP_PACKAGE, "--bin", APP_NAME, "--profile", profile])


def versioned_artifact_name(prefix: str, extension: str) -> str:
    return f"{prefix}-v{get_version()}-{get_system_name()}-{get_arch()}.{extension}"


def windows_zip_name() -> str:
    return versioned_artifact_name("minnowsnap", "zip")


def macos_dmg_name() -> str:
    return versioned_artifact_name(APP_NAME, "dmg")


def prune_windows_bundle(bundle_dir: Path) -> int:
    to_remove: set[Path] = set()
    for pattern in WINDOWS_DEBUG_GLOBS:
        to_remove.update(bundle_dir.rglob(pattern))

    removed_count = 0
    for path in sorted(to_remove, key=lambda p: len(p.parts), reverse=True):
        if path.exists():
            path.unlink(missing_ok=True)
            removed_count += 1
    return removed_count


def dist_windows(options: DistOptions) -> None:
    cargo_build(options.cargo_profile)

    deploy_dir, bundle_dir = clean_deploy_dir(APP_NAME)
    target_exe = get_target_dir(options.cargo_profile) / f"{APP_NAME}.exe"
    if not target_exe.exists():
        fail(f"Error: {target_exe} not found")

    bundle_exe = bundle_dir / f"{APP_NAME}.exe"
    shutil.copy2(target_exe, bundle_exe)

    before_prune = get_dir_size(bundle_dir)
    removed = prune_windows_bundle(bundle_dir)
    print_action("Slimming", f"removed {removed} debug artifacts")
    maybe_compress_exe_with_upx(bundle_exe, enabled=options.upx, aggressive=options.upx_aggressive)
    after_prune = get_dir_size(bundle_dir)

    print_action("Slimming", f"bundle size {format_size(before_prune)} -> {format_size(after_prune)}")
    print_bundle_report(bundle_dir)

    zip_name = windows_zip_name()
    zip_path = deploy_dir / zip_name
    print_action("Packaging", zip_name)
    create_zip(bundle_dir, zip_path, use_lzma=options.zip_lzma)
    print_action("Finished", f"output: {zip_path}")


def sign_macos_bundle(bundle_path: Path) -> None:
    """Sign the .app with the stable local identity via scripts/macos-sign.sh.

    cargo-bundle only leaves an ad-hoc signature, whose designated requirement
    is the per-build cdhash. macOS TCC then re-prompts for Screen Recording on
    every rebuild. macos-sign.sh signs with a persistent self-signed identity
    so the grant survives rebuilds. Signing failure is non-fatal: the DMG is
    still usable, it just re-prompts for permission like the old ad-hoc flow.
    """
    sign_script = SCRIPT_DIR / "macos-sign.sh"
    if not sign_script.exists():
        print_action("Warning", f"{sign_script.name} not found; skipping stable signing")
        return

    print_action("Signing", "bundle with stable local identity")
    try:
        subprocess.run([str(sign_script), str(bundle_path)], cwd=PROJECT_ROOT, check=True)
    except subprocess.CalledProcessError:
        print_action("Warning", "stable signing failed; bundle keeps its ad-hoc signature")


def dist_macos() -> None:
    print_action("Building", "bundle (cargo bundle)")
    run_command(["cargo", "bundle", "--manifest-path", str(APP_MANIFEST), "--release"])

    bundle_path = get_target_dir("release") / "bundle" / "osx" / f"{APP_NAME}.app"
    if not bundle_path.exists():
        fail(f"Error: {bundle_path} not found")

    sign_macos_bundle(bundle_path)

    ensure_clean_dir(DEPLOY_DIR)
    dmg_path = DEPLOY_DIR / macos_dmg_name()

    print_action("Packaging", "DMG installer (create-dmg)")
    if not shutil.which("create-dmg"):
        fail("Error: create-dmg not found. Please install it (brew install create-dmg).")

    volicon = APP_MANIFEST.parent / "assets_icons" / "icon.icns"
    volicon_path = PROJECT_ROOT / volicon
    cmd = [
        "create-dmg",
        "--volname", f"{APP_NAME} Installer",
        "--window-pos", "200", "120",
        "--window-size", "600", "400",
        "--icon-size", "100",
        "--icon", f"{APP_NAME}.app", "175", "190",
        "--hide-extension", f"{APP_NAME}.app",
        "--app-drop-link", "425", "190",
        str(dmg_path),
        str(bundle_path),
    ]
    if volicon_path.exists():
        cmd[1:1] = ["--volicon", str(volicon_path)]

    run_command(cmd)
    print_action("Finished", f"output: {dmg_path}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=f"Build and package {APP_NAME}")
    parser.add_argument(
        "--upx",
        action="store_true",
        help=f"Enable UPX compression for {APP_NAME}.exe",
    )
    parser.add_argument(
        "--upx-aggressive",
        action="store_true",
        help="Use aggressive UPX mode (smaller, but less stable)",
    )
    return parser.parse_args()


def main() -> None:
    options = build_options(parse_args())
    system = platform.system()
    if system == "Windows":
        dist_windows(options)
    elif system == "Darwin":
        dist_macos()
    else:
        fail(f"Unsupported OS: {system}")


if __name__ == "__main__":
    main()
