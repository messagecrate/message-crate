import fs from "fs";
import path from "path";
import { parse } from "smol-toml";

import { currentAccountId } from "./accountScope";

const DEFAULT_DB = "data/vault.db";
const DEFAULT_DATA_DIR = "data";
const DEFAULT_ASSETS_DIR = "assets";
const DEFAULT_ASSETS_CONVERTED_DIR = "assets_converted";

// Every path built from process.cwd() carries /*turbopackIgnore: true*/. The
// build's output file tracing otherwise reads a path joined from the working
// directory as possibly any file in the project, traces all of web-next/, and
// warns. These paths name the config and data files read at run time, which
// the standalone output never ships.

/** Repo root (parent of web-next/), detected via config/config.toml. */
export function repoRoot(): string {
  const cwd = process.cwd();
  if (fs.existsSync(path.join(/*turbopackIgnore: true*/ cwd, "config", "config.toml"))) {
    return cwd;
  }
  const parent = path.resolve(/*turbopackIgnore: true*/ cwd, "..");
  if (fs.existsSync(path.join(/*turbopackIgnore: true*/ parent, "config", "config.toml"))) {
    return parent;
  }
  return parent;
}

export function configTomlPath(): string {
  return path.join(/*turbopackIgnore: true*/ repoRoot(), "config", "config.toml");
}

function resolveConfiguredPath(
  configured: string | undefined,
  fallback: string,
): string {
  const rel = configured?.trim() || fallback;
  if (path.isAbsolute(rel)) return rel;
  return path.join(/*turbopackIgnore: true*/ repoRoot(), rel);
}

export type AccountAssetDirs = {
  assetsDir: string;
  assetsConvertedDir: string;
};

type RawConfig = {
  paths?: {
    db?: string;
    data_dir?: string;
    assets_dir?: string;
    assets_converted_dir?: string;
  };
};

function loadRawConfig(): RawConfig {
  const configPath = configTomlPath();
  if (!fs.existsSync(configPath)) {
    return {};
  }
  try {
    const text = fs.readFileSync(configPath, "utf8");
    return parse(text) as RawConfig;
  } catch {
    return {};
  }
}

export function dbPath(): string {
  const fromEnv = process.env.VAULT_DB?.trim();
  if (fromEnv) {
    return path.isAbsolute(fromEnv) ? fromEnv : path.join(/*turbopackIgnore: true*/ repoRoot(), fromEnv);
  }
  const cfg = loadRawConfig();
  return resolveConfiguredPath(cfg.paths?.db, DEFAULT_DB);
}

/** Parent of vault.db — better-sqlite3 fails if this directory is missing. */
export function ensureDbParentDir(): string {
  const file = dbPath();
  const dir = path.dirname(file);
  fs.mkdirSync(dir, { recursive: true });
  return file;
}

export function dataDir(): string {
  const fromEnv = process.env.VAULT_DATA_DIR?.trim();
  if (fromEnv) {
    return path.isAbsolute(fromEnv) ? fromEnv : path.join(/*turbopackIgnore: true*/ repoRoot(), fromEnv);
  }
  const cfg = loadRawConfig();
  return resolveConfiguredPath(cfg.paths?.data_dir, DEFAULT_DATA_DIR);
}

export function accountDataDir(accountId: string): string {
  return path.join(/*turbopackIgnore: true*/ dataDir(), accountId);
}

export function assetsDirName(): string {
  return loadRawConfig().paths?.assets_dir?.trim() || DEFAULT_ASSETS_DIR;
}

export function assetsConvertedDirName(): string {
  return (
    loadRawConfig().paths?.assets_converted_dir?.trim() ||
    DEFAULT_ASSETS_CONVERTED_DIR
  );
}

/**
 * One account's attachment folders. The server keeps one folder of originals
 * and one of previews per account, shared by every import source:
 *   data/<account_id>/<assets_dir>
 *   data/<account_id>/<assets_converted_dir>
 */
export function accountAssetDirs(accountId = currentAccountId()): AccountAssetDirs {
  const account = accountDataDir(accountId);
  return {
    assetsDir: path.join(/*turbopackIgnore: true*/ account, assetsDirName()),
    assetsConvertedDir: path.join(/*turbopackIgnore: true*/ account, assetsConvertedDirName()),
  };
}
