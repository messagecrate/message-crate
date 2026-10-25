/**
 * The signed-in account as `/v1` describes it: profile, sources, imports,
 * storage. Replaces `accounts.ts`, `accountProfile.ts`, `paths.ts` and
 * `storageStats.ts` for reads.
 *
 * The server has no `/v1/account` singleton: an account reads its own row as
 * `/v1/accounts/{id}`, with the id `GET /v1/session` names.
 */
import type { StorageUsage, VaultImportListItem } from "@/lib/storageTypes";

import { memo, vaultAll, vaultJson, type Schemas } from "./client";

export type Profile = Schemas["Account"];
type Session = Schemas["Session"];

const PROFILE_TTL_MS = 5_000;

/** The session the token names: its account id, username and sources. */
async function loadSession(): Promise<Session> {
  return memo("session", PROFILE_TTL_MS, () => vaultJson<Session>("/v1/session"));
}

/** `/v1/accounts/{id}` for the signed-in account. */
async function ownAccountPath(): Promise<string> {
  const session = await loadSession();
  return `/v1/accounts/${session.account_id}`;
}

export async function loadProfile(): Promise<Profile> {
  return memo("profile", PROFILE_TTL_MS, async () =>
    vaultJson<Profile>(await ownAccountPath()),
  );
}

/** What the thread view shows for messages the account owner sent. */
export function ownerDisplayName(profile: Profile): string {
  return profile.preferred_name?.trim() || "Me";
}

/** True while the server says the account still owes its profile setup. */
export async function accountNeedsOnboarding(): Promise<boolean> {
  const profile = await loadProfile();
  return profile.must_set_up_profile;
}

/** Import source ids this account has messages from (`GET /v1/session`). */
export async function listSources(): Promise<string[]> {
  const session = await loadSession();
  return [...session.sources].sort();
}

/** Shape `GET /api/settings/account` answers with. */
export type SettingsAccount = {
  id: string;
  username: string;
  emails: Array<{ email: string; isPrimary: boolean }>;
  noPassword: boolean;
  hasApiToken: boolean;
  isDemo: boolean;
  isAdmin: boolean;
  preferredName: string | null;
  displayName: string;
  phones: string[];
};

export async function settingsAccount(): Promise<SettingsAccount> {
  const profile = await loadProfile();
  return {
    id: String(profile.account_id),
    username: profile.username,
    emails: profile.emails.map((email, i) => ({ email, isPrimary: i === 0 })),
    noPassword: !profile.has_password,
    hasApiToken: false,
    isDemo: profile.is_demo,
    isAdmin: profile.is_admin,
    preferredName: profile.preferred_name ?? null,
    displayName: ownerDisplayName(profile),
    phones: profile.phones.map((phone) => phone.address),
  };
}

/**
 * The account's Import Runs, newest first, from `/v1/accounts/{id}/imports`:
 * `/v1/imports` is the import pipeline's own route and needs the import
 * permission.
 */
export async function listVaultImports(): Promise<VaultImportListItem[]> {
  const rows = await vaultAll<Schemas["ImportRunSummary"]>(
    `${await ownAccountPath()}/imports`,
  );
  return rows.map((row) => ({
    id: row.id,
    source: row.source,
    tool: row.tool ?? null,
    mode: row.mode,
    status: row.status,
    startedAt: row.started_at,
    finishedAt: row.finished_at ?? null,
    messageCount: row.message_count,
    attachmentCount: row.attachment_count,
    bytesUploaded: row.bytes_uploaded,
  }));
}

export async function loadStorageUsage(): Promise<StorageUsage> {
  const usage = await vaultJson<Schemas["AccountStorage"]>(
    `${await ownAccountPath()}/storage`,
  );
  return {
    totalBytes: usage.total_bytes,
    attachmentCount: usage.attachment_count,
    topAttachments: usage.top_attachments.map((row) => ({
      id: row.id,
      originalName: row.original_name ?? null,
      mimeType: row.mime_type ?? null,
      sizeBytes: row.size_bytes,
      conversationId: row.conversation_id ?? null,
      conversationTitle: row.group_title ?? null,
      chatIdentifier: row.chat_identifier ?? null,
    })),
  };
}
