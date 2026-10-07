import crypto from "node:crypto";
import fs from "node:fs";

import Database from "better-sqlite3";

import { accountDataDir } from "./paths";
import { hashPassword, passwordsMatch, validatePasswordPlaintext } from "./password";
import { createAccountProfile } from "./accountProfile";
import { resolveHandleId } from "./handlesWrite";
import { openWritableVaultDb } from "./vaultSchema";

export const INVALID_CREDENTIALS = "Invalid user ID or password";

/**
 * An email address that means “you” in messages: an `account_handles` row
 * pointing at an email handle. Not used for sign-in.
 */
export type AccountEmail = {
  email: string;
};

export type Account = {
  /** `accounts.id`, an integer, as a string. */
  id: string;
  /** Sign-in user ID (stored as `accounts.username`). */
  username: string;
  /** Optional email handles used to recognize “you” in messages — not for login. */
  emails: AccountEmail[];
};

export type AccountSummary = {
  id: string;
  username: string;
};

type AccountRow = {
  id: number;
  username: string;
};

/** How long a session token lasts, as the server's `SESSION_TTL_SECS` has it. */
const SESSION_TTL_SECS = 30 * 24 * 60 * 60;

/** Ids below this belong to accounts the server makes itself (owner, demo). */
const FIRST_GENERATED_ACCOUNT_ID = 100;

function normalizeEmail(email: string): string {
  return email.trim().toLowerCase();
}

function rowToAccount(row: AccountRow, emails: AccountEmail[]): Account {
  return {
    id: String(row.id),
    username: row.username,
    emails,
  };
}

function openDb(): Database.Database {
  return openWritableVaultDb();
}

function friendlyDbError(err: unknown): Error {
  const message = err instanceof Error ? err.message : String(err);
  if (message.includes("UNIQUE constraint failed: accounts.username")) {
    return new Error("That user ID is already taken.");
  }
  if (err instanceof Error) return err;
  return new Error(message);
}

function findAccountIdByUsername(db: Database.Database, username: string): string | null {
  const row = db
    .prepare(`SELECT id FROM accounts WHERE username = ? COLLATE NOCASE`)
    .get(username) as { id: number } | undefined;
  return row ? String(row.id) : null;
}

/** Optional email handles for message recognition (not sign-in). */
function validateEmails(emails: AccountEmail[]): AccountEmail[] {
  const normalized = emails.map((entry) => ({ email: entry.email.trim() }));

  if (normalized.some((entry) => !entry.email)) {
    throw new Error("email addresses cannot be empty");
  }

  const seen = new Set<string>();
  for (const entry of normalized) {
    const key = normalizeEmail(entry.email);
    if (seen.has(key)) {
      throw new Error("duplicate email addresses are not allowed");
    }
    seen.add(key);
  }

  return normalized;
}

function readAccountEmails(db: Database.Database, accountId: string): AccountEmail[] {
  return db
    .prepare(
      `SELECT h.raw AS email
       FROM account_handles ah
       JOIN handles h ON h.id = ah.handle_id
       WHERE ah.account_id = ? AND h.handle_type = 'email'
       ORDER BY h.raw COLLATE NOCASE`,
    )
    .all(accountId) as AccountEmail[];
}

/** Replace the account's email handles; its phone handles stay as they are. */
function writeAccountEmails(
  db: Database.Database,
  accountId: string,
  emails: AccountEmail[],
): void {
  db.prepare(
    `DELETE FROM account_handles
     WHERE account_id = ?
       AND handle_id IN (SELECT id FROM handles WHERE handle_type = 'email')`,
  ).run(accountId);
  const insert = db.prepare(
    `INSERT OR IGNORE INTO account_handles (account_id, handle_id) VALUES (?, ?)`,
  );
  for (const entry of emails) {
    insert.run(accountId, resolveHandleId(db, accountId, entry.email, "email"));
  }
}

function getAccountRow(db: Database.Database, accountId: string): AccountRow | undefined {
  return db
    .prepare(`SELECT id, username FROM accounts WHERE id = ?`)
    .get(accountId) as AccountRow | undefined;
}

/**
 * The id a new account takes: the next one above both the reserved range and
 * the highest id the table has ever held (`sqlite_sequence`, kept because the
 * column is AUTOINCREMENT), so a deleted account's id is never reused.
 */
function nextAccountId(db: Database.Database): number {
  const row = db
    .prepare(`SELECT seq FROM sqlite_sequence WHERE name = 'accounts'`)
    .get() as { seq: number } | undefined;
  return Math.max(row?.seq ?? 0, FIRST_GENERATED_ACCOUNT_ID - 1) + 1;
}

const API_TOKEN_ALPHANUM =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/** Import API token: `mv-user-` + 32 letters/digits. */
export function generateApiToken(): string {
  const bytes = crypto.randomBytes(32);
  let suffix = "";
  for (const b of bytes) {
    suffix += API_TOKEN_ALPHANUM[b % API_TOKEN_ALPHANUM.length]!;
  }
  return `mv-user-${suffix}`;
}

export function hashApiToken(token: string): string {
  return crypto.createHash("sha256").update(token, "utf8").digest("hex");
}

export function accountHasApiToken(accountId: string): boolean {
  const db = openDb();
  try {
    const row = db
      .prepare(
        `SELECT COUNT(*) AS n FROM account_session_tokens WHERE account_id = ?`,
      )
      .get(accountId) as { n: number };
    return row.n > 0;
  } finally {
    db.close();
  }
}

/** Create or replace the token hash; returns plaintext once. */
export function rotateAccountApiToken(accountId: string): string {
  const db = openDb();
  try {
    const row = getAccountRow(db, accountId);
    if (!row) throw new Error("account not found");
    const token = generateApiToken();
    const tokenHash = hashApiToken(token);
    const createdAt = Math.floor(Date.now() / 1000);
    const expiresAt = createdAt + SESSION_TTL_SECS;
    db.prepare(
      `INSERT INTO account_session_tokens (account_id, token_hash, created_at, expires_at)
       VALUES (?, ?, ?, ?)
       ON CONFLICT(account_id) DO UPDATE SET
         token_hash = excluded.token_hash,
         created_at = excluded.created_at,
         expires_at = excluded.expires_at`,
    ).run(accountId, tokenHash, String(createdAt), String(expiresAt));
    return token;
  } finally {
    db.close();
  }
}

export function deleteAccountApiToken(accountId: string): void {
  const db = openDb();
  try {
    const row = getAccountRow(db, accountId);
    if (!row) throw new Error("account not found");
    db.prepare(`DELETE FROM account_session_tokens WHERE account_id = ?`).run(
      accountId,
    );
  } finally {
    db.close();
  }
}

export function listAccounts(): AccountSummary[] {
  const db = openDb();
  try {
    const rows = db
      .prepare(
        `SELECT id, username FROM accounts ORDER BY username COLLATE NOCASE`,
      )
      .all() as Array<{ id: string; username: string }>;

    return rows.map((row) => ({
      id: row.id,
      username: row.username,
    }));
  } finally {
    db.close();
  }
}

export function getAccount(accountId: string): Account | null {
  const db = openDb();
  try {
    const row = getAccountRow(db, accountId);
    if (!row) return null;
    const emails = readAccountEmails(db, accountId);
    return rowToAccount(row, emails);
  } finally {
    db.close();
  }
}

/** True when the account was created with no password (`password_hash` NULL). */
export function accountHasNoPassword(accountId: string): boolean {
  const db = openDb();
  try {
    const row = db
      .prepare(`SELECT password_hash FROM accounts WHERE id = ?`)
      .get(accountId) as { password_hash: string | null } | undefined;
    if (!row) return false;
    return row.password_hash == null || row.password_hash === "";
  } finally {
    db.close();
  }
}

/** Replace an account password, or pass null to enable passwordless sign-in. */
export async function setAccountPassword(
  accountId: string,
  password: string | null,
): Promise<void> {
  const passwordHash = password === null ? null : await hashPassword(password);
  const db = openDb();
  try {
    const result = db
      .prepare(`UPDATE accounts SET password_hash = ? WHERE id = ?`)
      .run(passwordHash, accountId);
    if (result.changes === 0) {
      throw new Error("account not found");
    }
  } finally {
    db.close();
  }
}

export function isUsernameTaken(username: string): boolean {
  const trimmed = username.trim();
  if (!trimmed) return false;
  const db = openDb();
  try {
    return findAccountIdByUsername(db, trimmed) != null;
  } finally {
    db.close();
  }
}

/**
 * Verify user ID + password. Returns the account on success, or null for any failure
 * (unknown user / wrong password) so callers can show a single error message.
 */
export async function authenticateAccount(
  username: string,
  password: string,
): Promise<Account | null> {
  const trimmed = username.trim();
  if (!trimmed) return null;

  const db = openDb();
  try {
    const row = db
      .prepare(
        `SELECT id, username, password_hash
         FROM accounts WHERE username = ? COLLATE NOCASE`,
      )
      .get(trimmed) as
      | (AccountRow & { password_hash: string | null })
      | undefined;
    if (!row) return null;

    const ok = await passwordsMatch(row.password_hash, password);
    if (!ok) return null;

    const emails = readAccountEmails(db, String(row.id));
    return rowToAccount(row, emails);
  } finally {
    db.close();
  }
}

export async function createAccount(input: {
  username: string;
  preferredName: string;
  phone: string;
  /** Plaintext password, or omit/null when creating a no-password account. */
  password?: string | null;
  noPassword?: boolean;
}): Promise<Account> {
  const username = input.username.trim();
  const preferredName = input.preferredName.trim();
  if (!username) throw new Error("user ID is required");
  if (!preferredName) throw new Error("display name is required");

  // Explicit noPassword, or omitted password (tests / legacy callers) → passwordless.
  const noPassword =
    input.noPassword === true ||
    input.password === null ||
    input.password === undefined;
  let passwordHash: string | null = null;
  if (!noPassword) {
    const password = input.password ?? "";
    const pwdErr = validatePasswordPlaintext(password);
    if (pwdErr) throw new Error(pwdErr);
    passwordHash = await hashPassword(password);
  }

  const db = openDb();
  try {
    const existingId = findAccountIdByUsername(db, username);
    if (existingId) {
      throw new Error("That user ID is already taken.");
    }

    const id = String(nextAccountId(db));

    try {
      db.prepare(
        `INSERT INTO accounts (id, username, password_hash)
         VALUES (?, ?, ?)`,
      ).run(id, username, passwordHash);
      createAccountProfile(db, id, {
        preferred_name: preferredName,
        phones: [input.phone],
      });
    } catch (err) {
      throw friendlyDbError(err);
    }

    return {
      id,
      username,
      emails: [],
    };
  } finally {
    db.close();
  }
}

export function saveAccount(
  accountId: string,
  patch: Partial<Pick<Account, "username" | "emails">>,
): Account {
  const db = openDb();
  try {
    const row = getAccountRow(db, accountId);
    if (!row) {
      throw new Error("account not found");
    }

    const currentEmails = readAccountEmails(db, accountId);
    const current = rowToAccount(row, currentEmails);

    const nextEmails =
      patch.emails !== undefined ? validateEmails(patch.emails) : current.emails;
    const next: Account = {
      id: accountId,
      username: patch.username?.trim() || current.username,
      emails: nextEmails,
    };

    if (!next.username) {
      throw new Error("user ID is required");
    }

    db.prepare(`UPDATE accounts SET username = ? WHERE id = ?`).run(
      next.username,
      accountId,
    );

    writeAccountEmails(db, accountId, next.emails);
    return next;
  } finally {
    db.close();
  }
}

export function deleteAccount(accountId: string): void {
  const db = openDb();
  try {
    const row = getAccountRow(db, accountId);
    if (!row) {
      throw new Error("account not found");
    }

    db.pragma("foreign_keys = ON");
    db.prepare(`DELETE FROM accounts WHERE id = ?`).run(accountId);
  } finally {
    db.close();
  }

  const accountPath = accountDataDir(accountId);
  if (fs.existsSync(accountPath)) {
    fs.rmSync(accountPath, { recursive: true, force: true });
  }
}

/** @deprecated Use getAccount(accountId) with session context. */
export function loadAccount(accountId: string): Account {
  const account = getAccount(accountId);
  if (!account) {
    throw new Error("account not found");
  }
  return account;
}
