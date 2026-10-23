#!/usr/bin/env python3
"""Record compiler/test time from build logs without collecting playback data."""
from pathlib import Path
import json
import os
import re
import sys

ANSI = re.compile(r"\x1b\[[0-9;]*m")

def seconds(value):
    return sum(float(number) * {"h": 3600, "m": 60, "s": 1}[unit]
               for number, unit in re.findall(r"([0-9.]+)([hms])", value))

def inspect_log(text):
    text = ANSI.sub("", text)
    builds = [seconds(item) for item in re.findall(r"Finished `[^`]+`[^\n]*?target\(s\) in ([^\n]+)", text)]
    tests = [{"passed": int(p), "failed": int(f), "ignored": int(i), "elapsedSeconds": float(t)}
             for p, f, i, t in re.findall(
                 r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored;[^\n]*?finished in ([0-9.]+)s", text)]
    tasks = [{"task": name.strip(), "count": int(count), "aggregateTaskSeconds": float(time)}
             for name, count, time in re.findall(r"^\s*([^\n|]+?) \((\d+) tasks?\)\s*\|\s*([0-9.]+) seconds", text, re.M)]
    return {"cargoBuildElapsedSeconds": builds, "testRuns": tests, "xcodeTasks": tasks}

def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "ios-artifacts")
    root.mkdir(parents=True, exist_ok=True)
    logs = {}
    for path in sorted(root.glob("*.log")):
        result = inspect_log(path.read_text(encoding="utf-8", errors="replace"))
        if any(result.values()):
            logs[path.name] = result
    report = {"sourceCommit": os.environ.get("GITHUB_SHA"),
              "cargoTargetDir": os.environ.get("CARGO_TARGET_DIR"),
              "testProfileOverrides": {k: v for k, v in os.environ.items() if k.startswith("CARGO_PROFILE_TEST_")},
              "releaseProfileOverrides": {k: v for k, v in os.environ.items() if k.startswith("CARGO_PROFILE_RELEASE_")},
              "xcodeTaskTimesAreAggregatedNotWallClock": True, "logs": logs}
    (root / "build-timings.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as output:
            output.write("\n### Compiler and test timings\n\n| Log | Cargo compilation (s) | Test execution (s) |\n|---|---:|---:|\n")
            for name, item in logs.items():
                output.write(f"| {name} | {sum(item['cargoBuildElapsedSeconds']):.2f} | {sum(run['elapsedSeconds'] for run in item['testRuns']):.2f} |\n")
            output.write("\nXcode task summaries are in build-timings.json; their summed task time is not elapsed build time.\n")
    print(f"Wrote compiler/test timings from {len(logs)} logs.")

if __name__ == "__main__":
    main()
