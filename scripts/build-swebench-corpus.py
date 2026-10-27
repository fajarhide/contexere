#!/usr/bin/env python3
"""#838. A second benchmark corpus, built from SWE-bench Verified.

Every figure VENG publishes comes from `bench-corpus/`, which is frozen on one
machine and whose payloads stay local (#704). A reader has to take the number on
trust. This builds a corpus anyone can rebuild byte for byte, because every input
is public and pinned: SWE-bench Verified names a repository and a base commit per
instance, and the instance list is written into the manifest.

**No model, no key, no container, no spend.** The commands are a fixed script, not
what an agent chose, so this measures compression on realistic payloads and says
nothing about task success. The arm that answers task success runs a real agent
against both `VENG_PASSTHROUGH` settings and costs money; it is deliberately not
here.

**Every command has to be deterministic given the commit**, or the corpus hash
changes per machine and the whole point is lost. That rules out `ls -la` (mode,
owner, timestamps), bare `grep -r` (directory order) and anything touching the
clock. Where order is not guaranteed the output is piped through `sort`.

Usage:

    python3 scripts/build-swebench-corpus.py --instances 40 --stride 12
    VENG_BENCH_CORPUS=swebench-corpus/traces.jsonl ./scripts/bench.sh

Output, under `swebench-corpus/`, which is not committed: the clones are large and
the file is rebuildable from the manifest's instance list.
"""

import argparse
import collections
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
import time
import urllib.request

ROWS = (
    "https://datasets-server.huggingface.co/rows"
    "?dataset=princeton-nlp/SWE-bench_Verified&config=default&split=test"
    "&offset={offset}&length={length}"
)

# The same size gates the other corpus discloses, so the two manifests can be
# read side by side.
BUCKETS = [(0, 264), (264, 2000), (2000, 10000), (10000, 1 << 62)]

# Where the repository sits in every recorded command, so a payload never carries
# this machine's home directory and two runs on different machines agree.
MOUNT = "/repo"


def get(url, tries=5):
    """The dataset endpoint answers 502 often enough to fail a whole build on it.

    Plain backoff rather than a dependency: the alternative is a rebuild that
    dies twenty minutes of cloning in, which is what happened the first time.
    """
    for attempt in range(tries):
        try:
            with urllib.request.urlopen(url, timeout=60) as r:
                return r.read()
        except Exception as e:
            if attempt == tries - 1:
                raise
            wait = 2**attempt
            print(f"  {e}, retrying in {wait}s", file=sys.stderr)
            time.sleep(wait)


def fetch_all():
    """Every row, so a selection can be made by name rather than by position."""
    rows = []
    while len(rows) < 500:
        url = ROWS.format(offset=len(rows), length=min(100, 500 - len(rows)))
        page = json.loads(get(url))["rows"]
        if not page:
            break
        rows.extend(row["row"] for row in page)
    return rows


def fetch_by_id(ids):
    """The rows a previous build used, in the order that build used them.

    Selecting by position is not a pinning: the dataset is live, and a row edited
    or reordered upstream changes what `--instances 40 --stride 12` picks while
    the manifest still claims the same source (PR #841 review). The instance list
    in the manifest is the input, so a rebuild reads it rather than re-deriving
    the same guess.
    """
    by_id = {r["instance_id"]: r for r in fetch_all()}
    missing = [i for i in ids if i not in by_id]
    if missing:
        sys.exit(f"instances no longer in the dataset: {', '.join(missing)}")
    return [by_id[i] for i in ids]


def fetch_instances(count, offset, stride):
    """Instance rows from the public dataset endpoint. No key, no `datasets`.

    `stride` spreads the pick across the 500. The set is ordered by instance id,
    so the first 20 rows are almost all one repository and a corpus built from
    them would measure astropy rather than Python projects.
    """
    need = offset + count * max(1, stride)
    rows = []
    while len(rows) < need:
        want = min(100, need - len(rows))
        url = ROWS.format(offset=len(rows), length=want)
        page = json.loads(get(url))["rows"]
        if not page:
            break
        rows.extend(row["row"] for row in page)
    return rows[offset :: max(1, stride)][:count]


def run(cmd, cwd):
    """stdout and stderr as the agent would see them, joined and never raised on."""
    p = subprocess.run(
        ["bash", "-c", cmd],
        cwd=cwd,
        capture_output=True,
        text=True,
        errors="replace",
    )
    return p.stdout + p.stderr


def clone(repo, cache):
    """One clone per repository, reused across its instances.

    `--filter=blob:none` because the script checks out arbitrary base commits and
    a shallow clone cannot reach them; the filter fetches file contents lazily and
    keeps the whole history addressable.
    """
    dest = os.path.join(cache, repo.replace("/", "__"))
    if not os.path.isdir(os.path.join(dest, ".git")):
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        subprocess.run(
            [
                "git",
                "clone",
                "--filter=blob:none",
                "--quiet",
                f"https://github.com/{repo}.git",
                dest,
            ],
            check=True,
        )
    return dest


def patched_files(patch):
    """The files the gold patch touches, which is what a solver ends up reading."""
    return [m.group(1) for m in re.finditer(r"^--- a/(\S+)", patch, re.M)]


def symbol(patch, files):
    """An identifier worth grepping for, taken from the hunk headers.

    `@@ -1,2 +1,2 @@ def separable(transform):` is git naming the enclosing
    definition, which is the thing a solver searches for. Falls back to the first
    touched file's stem, which is always present.
    """
    for m in re.finditer(r"^@@[^@]*@@\s*(?:def|class)\s+(\w+)", patch, re.M):
        return m.group(1)
    for m in re.finditer(r"^[+-]\s*(?:def|class)\s+(\w+)", patch, re.M):
        return m.group(1)
    return os.path.splitext(os.path.basename(files[0]))[0] if files else "test"


def commands_for(inst):
    """The fixed exploration script, in the order a solver would issue it.

    Orientation, then search, then the files themselves, then the shape of the
    change. Every one is deterministic given the commit.
    """
    files = patched_files(inst["patch"])
    if not files:
        return []
    # Quoted, every one. These are paths and identifiers out of an HTTP response
    # being interpolated into `bash -c`, and a row carrying a shell
    # metacharacter would run whatever it liked as the person building the corpus
    # (PR #841 review). The shell stays because the pipes are the point.
    pkg = shlex.quote(files[0].split("/")[0])
    sym = shlex.quote(symbol(inst["patch"], files))
    paths = [shlex.quote(f) for f in files]
    cmds = [
        "git log --oneline -10",
        "git show --stat HEAD",
        f"find {pkg} -name '*.py' | sort | head -50",
        f"grep -rn {sym} --include='*.py' {pkg} | sort | head -40",
    ]
    # The files themselves, which is where the bytes are, capped so one enormous
    # instance cannot dominate the corpus.
    cmds += [f"cat {f}" for f in paths[:3]]
    # A window into the first one, the shape a `Read` with offset and limit takes.
    cmds.append(f"sed -n '1,120p' {paths[0]}")
    cmds.append("git diff --stat HEAD~1 HEAD")
    return cmds


def bucket_of(n):
    for i, (lo, hi) in enumerate(BUCKETS):
        if lo <= n < hi:
            return i
    return len(BUCKETS) - 1


def build(args):
    cache = os.path.expanduser(args.cache)
    os.makedirs(cache, exist_ok=True)
    os.makedirs(args.out, exist_ok=True)

    if args.from_manifest:
        ids = json.load(open(args.from_manifest))["instances"]
        instances = fetch_by_id(ids)
        print(f"rebuilding the {len(ids)} instances named in the manifest", file=sys.stderr)
    else:
        instances = fetch_instances(args.instances, args.offset, args.stride)
    print(f"{len(instances)} instances from SWE-bench Verified", file=sys.stderr)

    traces, entries = [], []
    class_bytes = collections.Counter()
    trace_id = 0
    used = []

    for inst in instances:
        iid, repo, base = inst["instance_id"], inst["repo"], inst["base_commit"]
        try:
            work = clone(repo, cache)
            subprocess.run(
                ["git", "-C", work, "checkout", "--quiet", "--force", base],
                check=True,
                capture_output=True,
            )
        except subprocess.CalledProcessError as e:
            print(f"  skip {iid}: {e}", file=sys.stderr)
            continue

        cmds = commands_for(inst)
        if not cmds:
            print(f"  skip {iid}: no files in patch", file=sys.stderr)
            continue
        used.append(iid)
        print(f"  {iid}: {len(cmds)} commands", file=sys.stderr)

        for seq, cmd in enumerate(cmds):
            payload = run(cmd, work).replace(work, MOUNT)
            if not payload:
                continue
            # Encoded once. `len()` on a `str` counts characters, and every size
            # here is compared against byte gates and against a Rust pipeline that
            # counts bytes, so a source file with one accented identifier would
            # undercount and could land in the wrong bucket (PR #841 review).
            size = len(payload.encode())
            trace_id += 1
            traces.append(
                {
                    "id": trace_id,
                    "seq": seq,
                    # The session is the instance: an agent works one instance at
                    # a time, and both engines are session scoped, so mixing them
                    # would invent cross-turn repetition that never happens.
                    "session": f"swebench-{iid}",
                    "project": f"swebench-{repo.replace('/', '-')}",
                    "agent": "claude_code",
                    "command": cmd,
                    "payload": payload,
                }
            )
            klass = cmd.split()[0]
            class_bytes[klass] += size
            entries.append(
                {
                    "id": trace_id,
                    "seq": seq,
                    "class": klass,
                    "bytes": size,
                    "bucket": bucket_of(size),
                    "sha256": hashlib.sha256(payload.encode()).hexdigest(),
                }
            )

    lines = "".join(json.dumps(t, sort_keys=True) + "\n" for t in traces)
    path = os.path.join(args.out, "traces.jsonl")
    with open(path, "w") as f:
        f.write(lines)

    manifest = {
        "schema": 1,
        "source": "princeton-nlp/SWE-bench_Verified",
        # The list is the corpus's identity: rerun with it and the payloads are
        # the same bytes, because every command is deterministic at that commit.
        "instances": used,
        "traces": len(traces),
        "sessions": len(used),
        "bytes": sum(len(t["payload"].encode()) for t in traces),
        "buckets": [list(b) for b in BUCKETS],
        "class_bytes": dict(sorted(class_bytes.items())),
        "entries": entries,
        "traces_sha256": hashlib.sha256(lines.encode()).hexdigest(),
    }
    # Hashed over `entries` alone, the way `build-bench-corpus.py` does it and
    # the way `scripts/bench.sh` recomputes it before it will replay anything.
    manifest["corpus_sha256"] = hashlib.sha256(
        json.dumps(manifest["entries"], sort_keys=True).encode()
    ).hexdigest()
    with open(os.path.join(args.out, "manifest.json"), "w") as f:
        json.dump(manifest, f, indent=1, sort_keys=True)

    print(
        f"wrote {path}: {len(traces)} traces, {manifest['bytes']:,} bytes, "
        f"{len(used)} instances",
        file=sys.stderr,
    )
    print(f"corpus_sha256 {manifest['corpus_sha256']}", file=sys.stderr)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--instances", type=int, default=20)
    ap.add_argument("--offset", type=int, default=0)
    ap.add_argument("--stride", type=int, default=1)
    ap.add_argument(
        "--from-manifest",
        help="rebuild exactly the instances a previous manifest names",
    )
    ap.add_argument("--out", default="swebench-corpus")
    ap.add_argument("--cache", default="~/.cache/veng-swebench")
    build(ap.parse_args())


if __name__ == "__main__":
    main()
