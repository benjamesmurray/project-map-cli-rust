#!/usr/bin/env python3
"""Builds each repository N times with one binary and records what the spec's item 6 asks for.

usage: measure.py BINARY LABEL OUTFILE [RUNS]   (BINARY is project-map-cli-rust of the version under test)
Each repository is built into a scratch --out, so nothing is written to the repository itself.
"""
import json, os, statistics, subprocess, sys, tempfile, time, shutil

binary, label, outfile = sys.argv[1:4]
runs = int(sys.argv[4]) if len(sys.argv) > 4 else 5
SELF = os.environ.get("SELF_SNAPSHOT", "")  # a git-archive of a516255, so the input never changes

REPOS = {
    "self": (SELF, [
        ("src/core/query_engine.rs", "QueryEngine"),
        ("src/core/toon.rs", "ToonFormatter"),
        ("src/core/orchestrator.rs", "Orchestrator"),
        ("src/core/graph.rs", "src.core.graph"),
        ("src/core/mod.rs", "src.core"),
    ]),
    "OST-Manager": ("/home/benmurray/Projects/OST-Manager", [
        ("src/ost_manager/config.py", "Settings"),
        ("src/ost_manager/ost/store.py", "src.ost_manager.ost.store"),
        ("src/ost_manager/ost/errors.py", "src.ost_manager.ost.errors"),
        ("src/ost_manager/contracts/workspace.py", "src.ost_manager.contracts.workspace"),
        ("src/ost_manager/llm/client.py", "src.ost_manager.llm.client"),
    ]),
    "wde": ("/opt/wde", [
        ("viz-portal/backend/app/logging.py", "viz-portal.backend.app.logging"),
        ("utils/physics-lab/optimizer/optimizer.py", "utils.physics-lab.optimizer.optimizer"),
        ("streams/wde-contracts/src/main/kotlin/com/wde/contracts/Bar.kt", "streams.wde-contracts.src.main.kotlin.com.wde.contracts.Bar"),
        ("streams/wde-contracts/src/main/kotlin/com/wde/contracts/PhaseLabelEvent.kt", "streams.wde-contracts.src.main.kotlin.com.wde.contracts.PhaseLabelEvent"),
        ("viz-portal/backend/app/api.py", "viz-portal.backend.app.api"),
    ]),
}

def one_build(root, symbols):
    out = tempfile.mkdtemp(prefix="pm-measure-")
    try:
        t = time.perf_counter()
        subprocess.run([binary, "build", "-r", root, "-o", out], check=True, capture_output=True)
        secs = time.perf_counter() - t
        idx = json.load(open(os.path.join(out, "latest", ".project-map.json")))
        nodes = idx["nodes"]
        imp = sum(1 for _, _, ty in idx["edges"] if ty == "Imports")
        ent = idx["imports"]
        blast = {}
        for path, sym in symbols:
            r = subprocess.run([binary, "blast", "-i", out, "-p", path, "-s", sym, "--format", "json", "-l", "1"],
                               capture_output=True, text=True)
            blast[f"{path}::{sym}"] = json.loads(r.stdout)["summary"].get("total", 0)
        return {
            "edges": len(idx["edges"]), "imports_edges": imp,
            "resolved": sum(1 for i in ent if i["resolved"]), "unresolved": sum(1 for i in ent if not i["resolved"]),
            "blast": blast, "seconds": round(secs, 3),
            "index_bytes": os.path.getsize(os.path.join(out, "latest", ".project-map.json")),
        }
    finally:
        shutil.rmtree(out, ignore_errors=True)

result = {"label": label, "binary": binary, "runs": runs, "repos": {}}
for name, (root, symbols) in REPOS.items():
    builds = [one_build(root, symbols) for _ in range(runs)]
    keys = ["edges", "imports_edges", "resolved", "unresolved"]
    spread = {k: sorted({b[k] for b in builds}) for k in keys}
    spread["blast"] = {s: sorted({b["blast"][s] for b in builds}) for s in builds[0]["blast"]}
    result["repos"][name] = {
        "builds": builds, "distinct_values": spread,
        "identical_across_builds": all(len(v) == 1 for k, v in spread.items() if k != "blast")
            and all(len(v) == 1 for v in spread["blast"].values()),
        "median_seconds": statistics.median(b["seconds"] for b in builds),
    }
json.dump(result, open(outfile, "w"), indent=1, sort_keys=True)
for name, r in result["repos"].items():
    print(name, "identical" if r["identical_across_builds"] else "VARIES", "median_s", r["median_seconds"])
    for k, v in r["distinct_values"].items():
        print("  ", k, v)
