#!/usr/bin/env node
/**
 * Check that every override of `noRestrictedImports` in web/biome.json keeps
 * the root rule's level and path list, less exactly one path: the one that
 * override lifts for its files. An override that sets the rule "off" is left
 * alone.
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

const failures = [];
for (const override of config.overrides ?? []) {
  const rule = override.linter?.rules?.style?.noRestrictedImports;
  // "off" lifts every restriction for the override's files, on purpose.
  if (rule === undefined || rule === "off") continue;
  const where = `the override for ${(override.includes ?? []).join(", ") || "no includes"}`;
  const paths = rule.options?.paths;
  if (paths === undefined) {
    failures.push(`${where} sets noRestrictedImports without paths; write it as "off" or with paths.`);
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
  if (missing.length !== 1) {
    failures.push(
      `${where} leaves out ${missing.length} of the root rule's paths (${missing.join(", ") || "none"}). An override leaves out exactly the one path it lifts; to lift two for the same files, write two overrides.`,
    );
  }
}

if (failures.length > 0) {
  console.error("web/biome.json: noRestrictedImports path lists disagree.");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log("web/biome.json: every noRestrictedImports override matches the root rule.");
