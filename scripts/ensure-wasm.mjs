#!/usr/bin/env node
// Builds the example's model package if it has not been built yet, so that
// `npm run dev` and `npm run typecheck` work on a fresh checkout.
import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";

const root = new URL("..", import.meta.url);
const outputs = ["pkg/todomvc_model.js", "plum_gen/todos.ts"];

if (outputs.every((file) => existsSync(new URL(`examples/todomvc/model/${file}`, root)))) {
  process.exit(0);
}

console.log("[plum] model package not built yet; running `npm run build:wasm`...");
const result = spawnSync("npm", ["run", "build:wasm"], { stdio: "inherit", cwd: root });
if (result.error) {
  console.error("[plum] could not run npm:", result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
