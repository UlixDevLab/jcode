#!/usr/bin/env python3
"""Generate a bounded, AST-only Graphify map of Jcode's primary runtime paths."""

from pathlib import Path
import json

from graphify.analyze import god_nodes, surprising_connections, suggest_questions
from graphify.build import build_from_json
from graphify.cluster import cluster, score_all
from graphify.export import to_json
from graphify.extract import extract
from graphify.report import generate


ROOT = Path(__file__).resolve().parents[1]
PATTERNS = (
    "src/main.rs",
    "src/cli/*.rs",
    "crates/jcode-app-core/src/lib.rs",
    "crates/jcode-app-core/src/server.rs",
    "crates/jcode-app-core/src/agent/*.rs",
    "crates/jcode-app-core/src/tool/mod.rs",
    "crates/jcode-app-core/src/tool/selfdev/*.rs",
    "crates/jcode-base/src/lib.rs",
    "crates/jcode-base/src/provider_catalog.rs",
    "crates/jcode-base/src/config/*.rs",
    "crates/jcode-base/src/session/*.rs",
    "crates/jcode-tui/src/lib.rs",
    "crates/jcode-tui/src/tui/mod.rs",
    "crates/jcode-protocol/src/lib.rs",
    "crates/jcode-provider-metadata/src/lib.rs",
    "crates/jcode-provider-metadata/src/catalog.rs",
)


def selected_files() -> list[Path]:
    files: set[Path] = set()
    for pattern in PATTERNS:
        files.update(path for path in ROOT.glob(pattern) if path.is_file())
    return sorted(files)


def main() -> int:
    files = selected_files()
    if not files:
        raise SystemExit("No Graphify source files matched the curated runtime scope")

    result = extract(files)
    graph = build_from_json(result)
    communities = cluster(graph)
    cohesion = score_all(graph, communities)
    labels = {community: f"Community {community}" for community in communities}
    gods = god_nodes(graph)
    surprises = surprising_connections(graph, communities)
    questions = suggest_questions(graph, communities, labels)
    relative_files = [str(path.relative_to(ROOT)) for path in files]
    detection = {
        "files": {"code": relative_files, "document": [], "paper": [], "image": []},
        "total_files": len(files),
        "total_words": 0,
    }

    output = ROOT / "graphify-out"
    output.mkdir(exist_ok=True)
    report = generate(
        graph,
        communities,
        cohesion,
        labels,
        gods,
        surprises,
        detection,
        {"input": 0, "output": 0},
        str(ROOT),
        suggested_questions=questions,
    )
    (output / "GRAPH_REPORT.md").write_text(report, encoding="utf-8")
    to_json(graph, communities, str(output / "graph.json"))
    (output / "scope.json").write_text(
        json.dumps({"mode": "curated-runtime-ast", "files": relative_files}, indent=2) + "\n",
        encoding="utf-8",
    )
    print(
        f"Graphify mapped {len(files)} files into "
        f"{graph.number_of_nodes()} nodes and {graph.number_of_edges()} edges."
    )
    print(f"Report: {output / 'GRAPH_REPORT.md'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
