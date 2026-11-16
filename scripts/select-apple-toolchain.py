"""Select a released Xcode bundle and verify its real SDKs before packaging."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess


def version_tuple(value):
    if not re.fullmatch(r"\d+(?:\.\d+)*", value):
        raise ValueError("Expected a numeric Apple version: " + value)
    return tuple(map(int, value.split(".")))


def matches_version(actual, requested):
    expected = version_tuple(requested)
    return version_tuple(actual)[:len(expected)] == expected


def output(command, env):
    return subprocess.check_output(command, env=env, text=True, stderr=subprocess.STDOUT).strip()


def select_toolchain(requested, ios_sdk=None, host_major=None, applications=Path("/Applications"), run=output):
    version_tuple(requested)
    host = run(["sw_vers", "-productVersion"], dict(os.environ))
    if host_major is not None and version_tuple(host)[0] != host_major:
        raise RuntimeError(f"Expected macOS {host_major}.x runner, got {host}")
    candidates = []
    for path in applications.glob("Xcode_*.app"):
        match = re.fullmatch(r"Xcode_(\d+(?:\.\d+)*)\.app", path.name)
        if match and matches_version(match[1], requested):
            candidates.append((version_tuple(match[1]), path))
    errors = []
    for _, app in sorted(candidates, reverse=True):
        env = dict(os.environ, DEVELOPER_DIR=str(app / "Contents/Developer"))
        try:
            xcode = run(["xcodebuild", "-version"], env)
            match = re.search(r"^Xcode (\d+(?:\.\d+)*)$", xcode, re.M)
            if not match or not matches_version(match[1], requested):
                raise RuntimeError("Bundle reports an unexpected Xcode version: " + xcode)
            iphoneos = run(["xcrun", "--sdk", "iphoneos", "--show-sdk-version"], env)
            macos = run(["xcrun", "--sdk", "macosx", "--show-sdk-version"], env)
            if ios_sdk and not matches_version(iphoneos, ios_sdk):
                raise RuntimeError(f"Expected iPhoneOS SDK {ios_sdk}, got {iphoneos}")
            sdk_path = run(["xcrun", "--sdk", "macosx", "--show-sdk-path"], env)
            record = {
                "macOSVersion": host, "runnerArchitecture": platform.machine(),
                "runnerImageOS": os.environ.get("ImageOS"),
                "runnerImageVersion": os.environ.get("ImageVersion"),
                "developerDirectory": env["DEVELOPER_DIR"],
                "xcodeVersion": match[1], "xcodeBuildDescription": xcode,
                "iphoneosSDKVersion": iphoneos, "macosSDKVersion": macos,
                "macosSDKPath": sdk_path,
            }
            fingerprint = {key: record[key] for key in ["xcodeBuildDescription", "iphoneosSDKVersion", "macosSDKVersion"]}
            record["fingerprint"] = hashlib.sha256(json.dumps(fingerprint, sort_keys=True).encode()).hexdigest()
            return env, record
        except (RuntimeError, ValueError, subprocess.SubprocessError, OSError) as error:
            errors.append(str(app) + ": " + str(error))
    raise RuntimeError("Required Xcode " + requested + " is unavailable or invalid. " + "; ".join(errors))


def probe_macos(env, record, architecture, directory, run=output):
    compiler = run(["xcrun", "--sdk", "macosx", "--find", "clang++"], env)
    source = directory / "toolchain-probe.cpp"
    source.write_text("#include <vector>\n#include <AudioToolbox/AudioToolbox.h>\nint main(){std::vector<int> a{1}; AudioStreamBasicDescription f{}; return a.front()!=1 || f.mSampleRate!=0;}\n")
    binary = directory / ("toolchain-probe-" + architecture)
    run([compiler, "-std=c++17", "-target", architecture + "-apple-macos12.0", "-isysroot", record["macosSDKPath"], str(source), "-framework", "AudioToolbox", "-o", str(binary)], env)
    description = run(["/usr/bin/file", str(binary)], env)
    if architecture not in description:
        raise RuntimeError("macOS compiler produced the wrong architecture: " + description)
    record["macosArchitectureProbe"] = description


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--xcode-version", required=True)
    parser.add_argument("--ios-sdk-version")
    parser.add_argument("--host-major", type=int)
    parser.add_argument("--record-dir", type=Path, required=True)
    parser.add_argument("--probe-macos-arch", choices=["x86_64", "arm64"])
    args = parser.parse_args()
    args.record_dir.mkdir(parents=True, exist_ok=True)
    try:
        env, record = select_toolchain(args.xcode_version, args.ios_sdk_version, args.host_major)
        if args.probe_macos_arch:
            probe_macos(env, record, args.probe_macos_arch, args.record_dir)
        (args.record_dir / "apple-toolchain.json").write_text(json.dumps(record, indent=2) + "\n")
        if os.environ.get("GITHUB_ENV"):
            with open(os.environ["GITHUB_ENV"], "a") as file:
                file.write("DEVELOPER_DIR=" + env["DEVELOPER_DIR"] + "\n")
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a") as file:
                file.write("available=true\nfingerprint=" + record["fingerprint"] + "\n")
        print(json.dumps(record, indent=2))
    except (RuntimeError, ValueError, subprocess.SubprocessError, OSError) as error:
        (args.record_dir / "apple-toolchain-error.txt").write_text(str(error) + "\n")
        raise SystemExit(str(error))


if __name__ == "__main__":
    main()
