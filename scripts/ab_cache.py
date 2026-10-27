#!/usr/bin/env python3
"""Paired A/B: does VENG cost prompt-cache hits, and what does a session bill with it on?

Each pair is the same prompt run twice through `claude -p`, seconds apart, once
with the hook live and once with `VENG_PASSTHROUGH=1`. The order alternates so a
warm cache cannot land on one arm every time. Every arm is recorded by the
session id the host returns, so the report reads the host's own transcript and
never VENG's tables.

It spends real tokens: about half a dollar a pair on a mid-size model.

    python3 scripts/ab_cache.py run --pairs 20 --model sonnet --out arms.json
    python3 scripts/ab_cache.py report arms.json

Run it from a clone of this repository, with VENG installed for Claude Code. The
workload reads two files twice each, because a workload with no repetition gives
the ledger nothing to do and measures nothing about it.

Both arms start the way a session starts for a user, so each resumes whatever
session state VENG kept. That is the same for both arms of a pair. `--fresh`
switches it off for both, which isolates the tool-result rewrite from it.
"""

import argparse
import glob
import json
import os
import subprocess
import sys
import time

PROMPT = (
    "Run each of these commands with the Bash tool, one at a time, in this order, "
    "and then reply with one sentence naming the file that appeared most often:\n"
    "1. cat CONTRIBUTING.md\n"
    "2. cat README.md\n"
    "3. cat CONTRIBUTING.md\n"
    "4. git log --oneline -40\n"
    "5. cat README.md\n"
    "6. git log --oneline -40\n"
    "Do not summarise the files. Do not run anything else."
)

TOKENS = ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens", "output_tokens")


def run_arm(pair, arm, model, timeout, fresh):
    # Set for the control and removed for the other arm, whatever the caller's
    # shell holds: an inherited VENG_PASSTHROUGH would switch both arms off and
    # the run would still spend its tokens.
    env = {k: v for k, v in os.environ.items() if k.upper() != "VENG_PASSTHROUGH"}
    if arm == "off":
        env["VENG_PASSTHROUGH"] = "1"
    if fresh:
        env["VENG_FRESH"] = "1"
    cmd = ["claude", "-p", "--output-format", "json"] + (["--model", model] if model else []) + [PROMPT]
    started = time.time()
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, env=env)
        result = json.loads(proc.stdout)
    except (subprocess.TimeoutExpired, json.JSONDecodeError) as err:
        # One lost arm loses its pair, not the run that is already paid for.
        print(f"  pair {pair} arm {arm}: dropped ({type(err).__name__})")
        return None
    if proc.returncode != 0 or not result.get("session_id"):
        print(f"  pair {pair} arm {arm}: exit {proc.returncode}, no session id")
        return None
    print(f"  pair {pair} arm {arm}: {result['session_id']}  ({time.time() - started:.0f}s)")
    return {
        "pair": pair,
        "arm": arm,
        "session_id": result["session_id"],
        "cost_usd": result.get("total_cost_usd"),
        "model": model,
    }


def run(args):
    arms = []
    for pair in range(1, args.pairs + 1):
        for arm in ("on", "off") if pair % 2 else ("off", "on"):
            row = run_arm(pair, arm, args.model, args.timeout, args.fresh)
            if row:
                arms.append(row)
            with open(args.out, "w") as fh:
                json.dump(arms, fh, indent=2)
    print(f"\nwrote {len(arms)} arm(s) to {args.out}")
    return 0 if arms else 1


def transcript(session_id):
    root = os.environ.get("CLAUDE_CONFIG_DIR") or os.path.expanduser("~/.claude")
    found = glob.glob(os.path.join(root, "projects", "**", session_id + ".jsonl"), recursive=True)
    return found[0] if found else None


def read_arm(path):
    """Billed tokens and delivered tool-result bytes for one session."""
    row = dict.fromkeys(TOKENS, 0)
    row.update(result_bytes=0, markers=0, requests=0)
    seen = set()
    with open(path, errors="replace") as fh:
        for line in fh:
            try:
                record = json.loads(line)
            except json.JSONDecodeError:
                continue
            message = record.get("message")
            if record.get("isSidechain") or not isinstance(message, dict):
                continue
            # The host repeats `usage` on every record of a request.
            # A record with no id cannot be matched to another, so it counts.
            request = record.get("requestId")
            if isinstance(message.get("usage"), dict) and (request is None or request not in seen):
                if request is not None:
                    seen.add(request)
                row["requests"] += 1
                for key in TOKENS:
                    row[key] += message["usage"].get(key) or 0
            content = message.get("content")
            for block in content if isinstance(content, list) else []:
                if isinstance(block, dict) and block.get("type") == "tool_result":
                    text = block.get("content")
                    if not isinstance(text, str):
                        text = "".join(p.get("text", "") for p in text or [] if isinstance(p, dict))
                    row["result_bytes"] += len(text.encode("utf-8"))
                    row["markers"] += text.count("[VENG:")
    return row


def report(args):
    with open(args.arms) as fh:
        arms = json.load(fh)
    pairs = {}
    for arm in arms:
        path = transcript(arm["session_id"])
        if not path:
            print(f"no transcript for {arm['session_id']}, dropping pair {arm['pair']}")
            continue
        row = read_arm(path)
        row["cost_usd"] = arm.get("cost_usd") or 0.0
        pairs.setdefault(arm["pair"], {})[arm["arm"]] = row
    pairs = {k: v for k, v in pairs.items() if len(v) == 2}
    if not pairs:
        print("no complete pair")
        return 1
    on = [p["on"] for p in pairs.values()]
    off = [p["off"] for p in pairs.values()]

    # The instrument before the result: an arm that ran without its condition
    # makes every number below it meaningless.
    print(f"{len(pairs)} complete pairs")
    print(f"markers per arm   on {min(a['markers'] for a in on)} to {max(a['markers'] for a in on)}"
          f"   off {min(a['markers'] for a in off)} to {max(a['markers'] for a in off)}")
    if min(a["markers"] for a in on) == 0 or max(a["markers"] for a in off) > 2:
        print("an arm ran without its condition; do not read the table below")

    print(f"\n{'':24}{'on':>14}{'off':>14}{'delta':>9}   lower with VENG")
    rows = [("tool_result bytes", "result_bytes"), ("cache_creation tokens", "cache_creation_input_tokens"),
            ("cache_read tokens", "cache_read_input_tokens"), ("output tokens", "output_tokens"),
            ("billed cost, USD", "cost_usd")]
    for label, key in rows:
        a, b = sum(x[key] for x in on), sum(x[key] for x in off)
        lower = sum(1 for p in pairs.values() if p["on"][key] < p["off"][key])
        delta = f"{100 * (a - b) / b:+.1f}%" if b else "n/a"
        number = "{:>14,.2f}" if key == "cost_usd" else "{:>14,.0f}"
        print(f"{label:24}" + number.format(a) + number.format(b) + f"{delta:>9}   {lower} of {len(pairs)} pairs")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="mode", required=True)
    runner = sub.add_parser("run", help="run the pairs and record each arm's session id")
    runner.add_argument("--pairs", type=int, default=20)
    runner.add_argument("--model", default=None, help="pin the model so the run can be repeated")
    runner.add_argument("--out", default="arms.json")
    runner.add_argument("--timeout", type=int, default=600)
    runner.add_argument("--fresh", action="store_true",
                        help="set VENG_FRESH=1 on both arms, so no arm resumes state an earlier one left")
    runner.set_defaults(func=run)
    reporter = sub.add_parser("report", help="read each arm's transcript and print the paired table")
    reporter.add_argument("arms")
    reporter.set_defaults(func=report)
    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
