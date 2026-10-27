#!/usr/bin/env bash
# #704. Replay the frozen corpus and record the result as a file in the repo.
#
# Every published figure before this was measured on `execution_traces`, which
# `TRACE_RETENTION_DAYS` deletes after 7 days. Two releases therefore reported
# numbers from two different corpora, and the delta between them mixed a code
# change with a corpus change. Nobody could say which.
#
# The corpus lives in `bench-corpus/` and is local: it holds real command output
# and is in `.git/info/exclude`. What lands in the repo is the measurement and a
# summary of what produced it, so a reader can see the composition and verify the
# corpus did not change between two releases, without ever holding the payloads.
#
#   docs/benchmarks/<version>.json   the measurement, diffable across releases
#
# Build the corpus first, once:
#   python3 docs/internal/runbooks/build-bench-corpus.py
#
# Then:
#   make bench
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# `CORPUS_DIR` picks which corpus to replay. The default is the frozen local one
# (#704); `swebench-corpus` is the public one anyone can rebuild (#838), and the
# artifact is named after it so the two never overwrite each other.
CORPUS_DIR="${CORPUS_DIR:-$ROOT/bench-corpus}"
CORPUS="$CORPUS_DIR/traces.jsonl"
MANIFEST="$CORPUS_DIR/manifest.json"
OUT_DIR="$ROOT/docs/benchmarks"
CORPUS_NAME="$(basename "$CORPUS_DIR")"
SUFFIX=""
[ "$CORPUS_NAME" = "bench-corpus" ] || SUFFIX="-$CORPUS_NAME"

if [ ! -f "$CORPUS" ]; then
  echo "no frozen corpus at $CORPUS" >&2
  echo "build it: python3 docs/internal/runbooks/build-bench-corpus.py" >&2
  echo "or the public one: python3 scripts/build-swebench-corpus.py" >&2
  exit 1
fi

VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
COMMIT="$(git -C "$ROOT" rev-parse --short HEAD)"
# Greptile on #704. The artifact records HEAD, so a run over a modified tree would
# claim it measured that commit. Recording the fact is enough; refusing would block
# the ordinary case of measuring a change before committing it.
if [ -n "$(git -C "$ROOT" status --porcelain)" ]; then DIRTY=true; else DIRTY=false; fi
LOG="$(mktemp)"
trap 'rm -f "$LOG"' EXIT

# #712. The head to head used to live only in a session's scrollback, so the
# README could claim VENG was the top arm while the last actual run said otherwise
# and nobody could check. An arm runs whenever its binary is on this machine, and
# what ran lands in the artifact next to our own figures.
#
# Cost: the competitor arms spawn a process per trace, so a full run is ~20 minutes
# against ~40 seconds for ours alone. `VENG_BENCH_ARMS=off` skips them when the
# question is only whether a change moved our own number.
declare -a ARMS_RUN=() ARMS_ABSENT=()
if [ "${VENG_BENCH_ARMS:-on}" != "off" ]; then
  # caveman is not on PATH and is not a bare `caveman`: the binary that compresses
  # is `caveman-engine`. See the caveman_out docstring for what naming a plausible
  # sibling produces instead, which is a silent zero.
  : "${VENG_BENCH_RTK:=$(command -v rtk || true)}"
  : "${VENG_BENCH_LEANCTX:=$(command -v lean-ctx || true)}"
  : "${VENG_BENCH_HEADROOM:=$(command -v headroom || true)}"
  : "${VENG_BENCH_CAVEMAN:=$([ -x "$HOME/.caveman/bin/caveman-engine" ] && echo "$HOME/.caveman/bin/caveman-engine" || true)}"
  for arm in RTK LEANCTX HEADROOM CAVEMAN; do
    var="VENG_BENCH_$arm"
    if [ -n "${!var:-}" ]; then
      export "$var"; ARMS_RUN+=("$arm")
    else
      # Greptile on #712. An arm nobody could find used to vanish without a trace,
      # so a three-arm table and a four-arm table were the same artifact. Recorded
      # rather than refused: a machine without the competitors installed still has
      # a legitimate reason to run this, and the artifact should say which it is.
      unset "$var" || true; ARMS_ABSENT+=("$arm")
    fi
  done
fi
echo "replaying $(wc -l < "$CORPUS" | tr -d ' ') traces against $VERSION ($COMMIT)"
echo "arms: ${ARMS_RUN[*]:-none}${ARMS_ABSENT:+ (absent: ${ARMS_ABSENT[*]})}"
VENG_BENCH_CORPUS="$CORPUS" \
  cargo test --release --test bench_replay -- --ignored --nocapture >"$LOG" 2>&1 || {
  tail -30 "$LOG" >&2
  exit 1
}

mkdir -p "$OUT_DIR"
# The figures come out of the replay's own output rather than being recomputed
# here, so this script cannot disagree with the harness it ran. A missing field
# is left null instead of defaulted: a benchmark that reports 0 where it failed
# to read is the failure mode this whole ticket is about.
python3 - "$LOG" "$MANIFEST" "$VERSION" "$COMMIT" "$OUT_DIR" "$DIRTY" "${ARMS_RUN[*]:-}" \
  "${ARMS_ABSENT[*]:-}" "$SUFFIX" <<'PY'
import json, os, re, sys

log, manifest_path, version, commit, out_dir, dirty, arms_run, arms_absent = sys.argv[1:9]
# The corpus names the file, never the version: `VENG 0.7.9-swebench-corpus` read
# as a release that does not exist in the generated footer (#838).
suffix = sys.argv[9] if len(sys.argv) > 9 else ""
text = open(log, errors="replace").read()
manifest = json.load(open(manifest_path))

# Greptile on #704. The report copies the manifest's `corpus_sha256`, so the
# manifest has to be proven to describe the file the replay just read. Without
# this, editing or rebuilding `traces.jsonl` without its manifest produces two
# measurements over different inputs carrying one identity, and their delta reads
# as a code change. That is the defect this whole ticket exists to remove, so it
# aborts rather than warns.
import hashlib

corpus_path = manifest_path.replace("manifest.json", "traces.jsonl")
with open(corpus_path, "rb") as fh:
    actual = hashlib.sha256(fh.read()).hexdigest()
expected = manifest.get("traces_sha256")
if expected is None:
    sys.exit(
        "manifest has no traces_sha256; rebuild it with "
        "docs/internal/runbooks/build-bench-corpus.py"
    )
if actual != expected:
    sys.exit(
        f"corpus does not match its manifest\n"
        f"  traces.jsonl {actual}\n"
        f"  manifest     {expected}\n"
        "rebuild the manifest before measuring anything"
    )

# And `corpus_sha256` itself, because that is the field the report publishes as
# the corpus identity. Checking only the payload file leaves a manifest whose
# entries or whose stated hash were edited independently, and the artifact would
# then carry a hash that describes neither. Recomputed the way the builder
# computes it, so the two cannot drift.
restated = hashlib.sha256(
    json.dumps(manifest["entries"], sort_keys=True).encode()
).hexdigest()
if restated != manifest.get("corpus_sha256"):
    sys.exit(
        f"manifest does not describe itself\n"
        f"  entries hash to {restated}\n"
        f"  corpus_sha256   {manifest.get('corpus_sha256')}\n"
        "rebuild the manifest before measuring anything"
    )


def num(pattern):
    m = re.search(pattern, text)
    return float(m.group(1)) if m else None


def whole(pattern):
    m = re.search(pattern, text)
    return int(m.group(1)) if m else None


def arms():
    """The head-to-head rows, parsed from the same output as everything else.

    Each row is `<label>  <raw> -> <out>  <pct>%`, printed only when that arm's
    binary was named. Labels are matched exactly rather than by shape, so a new row
    is a deliberate addition here and a renamed one fails the silence check below
    instead of quietly disappearing from the artifact.
    """
    labels = {
        "veng, filters only": "veng_filters",
        "veng, with ledger": "veng_ledger",
        "rtk pipe": "rtk",
        "rtk pipe + our ledger": "rtk_ledger",
        "caveman compress": "caveman",
        "caveman + our ledger": "caveman_ledger",
        "headroom dedup": "headroom",
        "lean-ctx compress": "lean_ctx",
    }
    out = {}
    for line in text.splitlines():
        m = re.match(r"^(\S.*?)\s{2,}(\d+) -> \s*(\d+)\s+([\d.]+)%", line)
        if not m:
            continue
        key = labels.get(m.group(1).strip())
        if key:
            out[key] = {
                "input_bytes": int(m.group(2)),
                "output_bytes": int(m.group(3)),
                "saved_pct": float(m.group(4)),
            }
    return out


def by_class():
    """The per-class table the replay prints, parsed rather than recomputed.

    Class names come from `trace_class` in the harness, which is a fixed set of
    generic buckets (`file read`, `search`, `git`, `infra`, `build and test`,
    `other`), so unlike command classes they carry nothing local and need no
    allowlist. Rows are read by shape; a table that stops printing a column shows
    up as an empty dict and fails the missing-field check below.
    """
    block = re.search(
        r"by command class.*?\n(.*?)\nledger arm:", text, re.S
    )
    if not block:
        return {}
    out = {}
    for line in block.group(1).splitlines():
        m = re.match(
            r"^(\S.*?)\s{2,}(\d+)\s+(\d+)\s+([\d.]+)%\s+([\d.]+)%"
            r"\s+([\d.]+)%\s+([\d.]+)%",
            line,
        )
        if not m:
            continue
        out[m.group(1).strip()] = {
            "calls": int(m.group(2)),
            "input_bytes": int(m.group(3)),
            "filters_pct": float(m.group(4)),
            "with_ledger_pct": float(m.group(5)),
            "repetition_available_pct": float(m.group(6)),
            "capture_rate_pct": float(m.group(7)),
        }
    return out


# A command class is the basename of whatever ran, so the corpus knows the names
# of local scripts, and the first run of this put a client's name into the
# artifact. Only names on this list are published; everything else is summed into
# `[other]`.
#
# **An allowlist, and deliberately not a denylist.** A denylist would have to
# carry the names it protects against, which means committing the whole client
# list to a public repository in one file, in git history, forever. That is a
# larger leak than the one it prevents. An allowlist can only under-disclose, and
# under-disclosing here costs nothing: the classes worth reporting are the generic
# tools any reader would recognise, and a name nobody outside this machine can
# interpret was never disclosure in the first place.
#
# Adding a name is a deliberate act. If a real tool keeps landing in `[other]`,
# put it here rather than reaching for a pattern that admits whole shapes.
PUBLISHABLE = set(
    """
    apt awk az bash bun bundle cargo cat cd chmod cp curl cut deno df diff dig
    docker du echo env find gh git go gradle grep head helm httpie ip jq kubectl
    ls make mkdir mv nc netstat next nmap node npm npx openssl pip pnpm ps psql
    pytest python python3 rg rm rsync ruby sed seq sh sort ssh sudo tail tar tee
    terraform terragrunt tr uname uniq uv vercel wc wget xargs yarn yq zsh
    """.split()
)

CLASS_FLOOR = 0.005
corpus_bytes = manifest["bytes"] or 1
named, other = {}, 0
for cls, b in manifest["class_bytes"].items():
    if cls in PUBLISHABLE and b / corpus_bytes >= CLASS_FLOOR:
        named[cls] = b
    else:
        other += b
if other:
    named["[other]"] = other

report = {
    "version": version,
    "commit": commit,
    # #760. The replay used to build its ledger without ever naming the command
    # that produced the payload, so every source-aware rule was inert here: the
    # `from` clause (#622), the line budget (#735, #742, #750) and the re-run
    # wording (#755). Artifacts written before this field exists were measured by
    # that harness, and their fold rate is an upper bound on production rather
    # than a measurement of it. A delta across the boundary is a harness change
    # and a code change added together, which is exactly what #704 exists to
    # prevent, so the boundary is recorded rather than explained in a release
    # note nobody will have to hand.
    "harness": {"ledger_knows_the_command": True},
    # True means the working tree carried uncommitted changes, so `commit` names
    # the nearest commit and not the source that was measured.
    "dirty_tree": dirty == "true",
    "corpus": {
        "sha256": manifest["corpus_sha256"],
        "traces": manifest["traces"],
        "bytes": manifest["bytes"],
        "sessions": manifest["sessions"],
        # Byte share per command class, so a reader can see what the corpus is
        # made of and judge whether a figure generalises to their own work. Only
        # classes at or above CLASS_FLOOR are named; see above for why.
        "class_bytes": named,
        "class_floor": CLASS_FLOOR,
        "buckets": manifest["buckets"],
    },
    "result": {
        "net_savings_pct": num(r"NET SAVINGS:\s+([\d.]+)%"),
        "traces_replayed": whole(r"corpus:\s+(\d+) traces"),
        "repeated_bytes_pct": num(r"repeated bytes handed to the ledger: \d+ \(([\d.]+)%"),
        "ledger_claimed_pct": num(r"claimed by the ledger:\s+\d+ \(([\d.]+)%"),
        # #842. The page's hand-written headline quotes these four beside the two
        # above, and only the two were in the artifact, so a test could hold half
        # the sentence and the other half drifted a release without anyone seeing.
        # Recorded here so the whole claim is checkable rather than half of it.
        "saved_nothing_pct": num(r"saved nothing:\s+([\d.]+)%"),
        "grew_calls": whole(r"saved nothing:.*\+ (\d+) grew"),
        "shrank_pct": num(r"actually shrank:\s+([\d.]+)%"),
        "tokens_before": whole(r"\ntokens:\s+(\d+) ->"),
        "tokens_after": whole(r"\ntokens:\s+\d+ -> (\d+)"),
        # #708. Per class, and `captured` is the one that does not move with the
        # workload: on this corpus file reads save 4.5% where a week of large
        # repeated reads read 89.6%, while the share of available repetition the
        # ledger takes stays in a narrow band. A savings percentage describes the
        # week; this describes VENG.
        "by_class": by_class(),
        # #712. The head to head, recorded rather than left in a scrollback. Absent
        # arms are absent from the dict; an arm that was named and produced no row
        # aborts below rather than being published as a zero.
        "arms": arms(),
        # Greptile on #712. Which competitors this machine could not find, so a
        # table with three arms cannot be read as a complete four-arm comparison.
        "arms_absent": arms_absent.split(),
    },
}
required = dict(report["result"])
# The replay prints the head-to-head block only when at least one competitor was
# named, so with `VENG_BENCH_ARMS=off` an empty `arms` is the correct answer rather
# than a failed read. Required whenever an arm was asked for, and only then.
if not arms_run.split():
    required.pop("arms", None)
# `VENG_BENCH_NO_TOKENS=1` tells the replay to skip tokenization, which is the slow
# half and a documented way to run this. Absent totals are the correct answer there,
# and requiring them aborted the whole bench without writing an artifact, for a
# measurement that is about bytes (PR #843 review).
if os.environ.get("VENG_BENCH_NO_TOKENS"):
    required.pop("tokens_before", None)
    required.pop("tokens_after", None)
missing = [k for k, v in required.items() if v is None or v == {}]
if missing:
    sys.exit(f"replay output did not carry: {', '.join(missing)}")

# #711's lesson, applied to the artifact. A named arm that prints nothing is what
# let headroom sit out every published comparison while the table still read as a
# four-arm result. Silence is a failure, never a loss.
EXPECTED = {
    "RTK": ("rtk", "rtk_ledger"),
    "LEANCTX": ("lean_ctx",),
    "HEADROOM": ("headroom",),
    "CAVEMAN": ("caveman", "caveman_ledger"),
}
silent = [
    f"{arm} ({', '.join(k for k in keys if k not in report['result']['arms'])})"
    for arm in arms_run.split()
    for keys in [EXPECTED[arm]]
    if any(k not in report["result"]["arms"] for k in keys)
]
if silent:
    sys.exit(f"named arms produced no row: {'; '.join(silent)}")

# Greptile on #712, and the sharper half of the same defect. Every competitor arm
# returns the raw payload on any failure, `rtk_out` from three separate paths, so a
# binary that cannot spawn, cannot parse its arguments, or writes nothing does not go
# silent. It emits a full table of unchanged bytes and lands as a tidy 0.0% row. That
# is what `caveman tools compress` was doing, and a presence check cannot see it.
#
# headroom is deliberately not probed here: its arm panics outright when it produces
# no number (#711), and a headroom that deduped nothing would read as our own
# filters-only figure rather than as zero, so a zero test would never fire on it.
#
# Greptile again, on the first version of this check: it read `saved_pct`, which is
# rounded to one decimal, so a competitor that genuinely saved under 0.05% would have
# been failed as broken. The bytes are in the artifact unrounded, and `output ==
# input` says exactly what this needs to say, that not one byte moved across the whole
# corpus. rtk's own claimed count cannot stand in for it: `rtk_claimed` counts
# commands *our* `rtk_filter` mapped, whether or not their binary then did anything.
PROBE = {"RTK": "rtk", "LEANCTX": "lean_ctx", "CAVEMAN": "caveman"}
inert = [
    f"{arm} ({key} returned all {report['result']['arms'][key]['input_bytes']:,} bytes)"
    for arm in arms_run.split()
    for key in [PROBE.get(arm)]
    if key
    and (a := report["result"]["arms"].get(key))
    and a["output_bytes"] == a["input_bytes"]
]
if inert:
    sys.exit(
        "named arms changed nothing on any trace, which is a broken binary and not "
        f"a measured loss: {'; '.join(inert)}"
    )

path = f"{out_dir}/{version}{suffix}.json"
with open(path, "w") as fh:
    json.dump(report, fh, indent=2, sort_keys=True)
    fh.write("\n")
print(f"\nwrote {path}")
for k, v in report["result"].items():
    print(f"  {k:<22}{v}")
print(f"  corpus_sha256         {report['corpus']['sha256'][:16]}...")
PY
