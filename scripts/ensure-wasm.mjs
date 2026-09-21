#!/usr/bin/env node
// Builds the example's wasm module on demand if it has not been built yet.
// Keeps `npm run dev` and `npm run typecheck` one-command on a fresh checkout.
import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";

const root = new URL("..", import.meta.url);
const marker = new URL("examples/todomvc/model/pkg/todomvc_model.js", root);

if (existsSync(marker)) {
  process.exit(0);
}

console.log("[plum] wasm module not built yet; running `wasm-pack build`...");
const result = spawnSync(
  "wasm-pack",
  ["build", "examples/todomvc/model", "--target", "web", "--features", "plum"],
  { stdio: "inherit", cwd: root },
);
if (result.error) {
  console.error(
    '[plum] failed to run wasm-pack. Is it installed and on PATH? See README ("Required tooling").',
  );
  process.exit(1);
}
process.exit(result.status ?? 1);
