#!/usr/bin/env node
/**
 * Check that every override of `noRestrictedImports` in web/biome.json keeps
 * the root rule's level and path list, less what that override lifts for its
 * files: exactly one path, or every one of Tauri's packages (`@tauri-apps/`)
 * together, which only the override for the modules in web/src/lib/desktop/
 * may lift. An override that sets the rule "off" is left alone.
 *
 * Usage:
 *   node scripts/check-biome-restricted-imports.mjs
 *
 * Read-only. An override's `paths` replace the root rule's rather than adding
 * to them, and Biome refuses comments in biome.json, so each override that
 * lifts one restriction writes out every other one again. A path added to the
 * root rule and missed in an override is lifted for that override's files
 * without anyone meaning it (#2623). Runs in check-pr.sh and in CI's web job.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const configPath = path.join(root, "web", "biome.json");
const config = JSON.parse(fs.readFileSync(configPath, "utf8"));

const rootRule = config.linter?.rules?.style?.noRestrictedImports;
const rootPaths = rootRule?.options?.paths;
if (rootPaths === undefined) {
  console.error("web/biome.json: the root noRestrictedImports rule has no paths.");
  process.exit(1);
}

// Tauri's packages, which the desktop modules lift as one, and the files of
// the one override that may lift them: the modules in src/lib/desktop/, less
// their tests.
const tauriPaths = Object.keys(rootPaths).filter((name) => name.startsWith("@tauri-apps/"));
const desktopIncludes = ["src/lib/desktop/*.ts", "!src/lib/desktop/*.test.ts"];

const failures = [];
for (const override of config.overrides ?? []) {
  const rule = override.linter?.rules?.style?.noRestrictedImports;
  // "off" lifts every restriction for the override's files, on purpose.
  if (rule === undefined || rule === "off" || rule.level === "off") continue;
  const where = `the override for ${(override.includes ?? []).join(", ") || "no includes"}`;
  const paths = rule.options?.paths;
  if (paths === undefined) {
    failures.push(`${where} sets noRestrictedImports without paths. Write it as "off" or with paths.`);
    continue;
  }
  if (rule.level !== rootRule.level) {
    failures.push(`${where} sets level "${rule.level}", and the root rule sets "${rootRule.level}".`);
  }
  for (const [name, setting] of Object.entries(paths)) {
    if (!(name in rootPaths)) {
      failures.push(`${where} restricts "${name}", which the root rule does not.`);
    } else if (!isDeepStrictEqual(setting, rootPaths[name])) {
      failures.push(`${where} writes "${name}" differently from the root rule.`);
    }
  }
  const missing = Object.keys(rootPaths).filter((name) => !(name in paths));
  const liftsTauri = tauriPaths.length > 0 && isDeepStrictEqual(missing, tauriPaths);
  if (liftsTauri && !isDeepStrictEqual(override.includes, desktopIncludes)) {
    failures.push(
      `${where} lifts every @tauri-apps/ path, which only the override for ${desktopIncludes.join(", ")} may do.`,
    );
  } else if (missing.length !== 1 && !liftsTauri) {
    failures.push(
      `${where} leaves out ${missing.length} of the root rule's paths (${missing.join(", ") || "none"}). An override leaves out exactly the one path it lifts, or, for src/lib/desktop/, every @tauri-apps/ path together. Lifting any other set of paths for the same files needs a change to this check.`,
    );
  }
}

if (failures.length > 0) {
  console.error("web/biome.json: noRestrictedImports path lists disagree.");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("web/biome.json: every noRestrictedImports override matches the root rule.");
