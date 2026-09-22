#!/usr/bin/env node
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { exportLite } from "../renderer/export-lite.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const template = resolve(here, "../renderer/renderer-template.html");

function parse(argv) {
  const [command, ...rest] = argv;
  let project = process.cwd();
  for (let index = 0; index < rest.length; index += 1) {
    if (rest[index] !== "--project" || !rest[index + 1]) {
      throw new Error("Usage: knowledge-os-lite <export|validate> --project <path>");
    }
    project = resolve(rest[++index]);
  }
  return { command, project };
}

function boardFile(project) {
  const file = join(project, ".opencode", "project-os.yaml");
  if (!existsSync(file)) throw new Error(`No project board at ${file}`);
  return file;
}

function validate(project) {
  const temporary = mkdtempSync(join(tmpdir(), "jcode-lite-kos-"));
  try {
    mkdirSync(join(temporary, ".opencode"), { recursive: true });
    copyFileSync(boardFile(project), join(temporary, ".opencode", "project-os.yaml"));
    exportLite(temporary, template);
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
  process.stdout.write("Knowledge OS board is valid.\n");
}

try {
  const { command, project } = parse(process.argv.slice(2));
  if (command === "export") {
    boardFile(project);
    exportLite(project, template);
  } else if (command === "validate") {
    validate(project);
  } else {
    throw new Error("Usage: knowledge-os-lite <export|validate> --project <path>");
  }
} catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exitCode = error.code ?? 3;
}
