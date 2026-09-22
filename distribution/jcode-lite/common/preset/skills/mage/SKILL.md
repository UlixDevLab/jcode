---
name: mage
description: >-
  Build, validate, and render Knowledge OS project boards with the native
  mage_board tool. Trigger: /mage, project board, knowledge graph,
  project-os, plan diagram, workflow map.
---

# Mage

Use this skill when a user wants a visual project plan, architecture board,
workflow map, or data-model graph.

1. Look for `.opencode/project-os.yaml` in the project root.
2. If it is missing, copy the bundled `project-os.schema.yaml` and
   `block-catalog.yaml` references from the Jcode Lite templates directory and
   create a board using `schema_version: project-os-schema/v2.2`.
3. Keep stable node IDs, typed nodes, explicit relations, and concise labels.
4. Call `mage_board` with `action: validate` before rendering.
5. Call `mage_board` with `action: export` to write
   `.opencode/project-os.html`. Set `open: true` when the user wants to inspect it.
6. Use `mage_board` with `action: status` to report freshness without changing files.

Do not invent dependencies merely to make a graph look connected. Treat the
bundled schema and block catalog as the source of truth.
