/**
 * The signed-in account as `/v1` describes it: profile, sources, imports,
 * storage. Replaces `accounts.ts`, `accountProfile.ts`, `paths.ts` and
 * `storageStats.ts` for reads.
 */
import type { StorageUsage, VaultImportListItem } from "@/lib/storageTypes";

import { memo, vaultJson, type Schemas } from "./client";

export type Profile = Schemas["AccountProfileResponse"];

const PROFILE_TTL_MS = 5_000;

export async function loadProfile(): Promise<Profile> {
  return memo("profile", PROFILE_TTL_MS, () =>
    vaultJson<Profile>("/v1/account/profile"),
  );
}

/** What the thread view shows for messages the account owner sent. */
export function ownerDisplayName(profile: Profile): string {
  return profile.preferred_name?.trim() || "Me";
}

/** True when preferred name or at least one phone is still missing. */
export async function accountNeedsOnboarding(): Promise<boolean> {
  const profile = await loadProfile();
  const name = profile.preferred_name?.trim() ?? "";
  return !name || profile.phones.length === 0;
}

/** Import source ids this account has messages from (`GET /v1/auth/check`). */
export async function listSources(): Promise<string[]> {
  return memo("sources", PROFILE_TTL_MS, async () => {
    const check = await vaultJson<Schemas["AuthCheckResponse"]>("/v1/auth/check");
    return [...check.sources].sort();
  });
}

/** Shape `GET /api/settings/account` answers with. */
export type SettingsAccount = {
  id: string;
  username: string;
  emails: Array<{ email: string; isPrimary: boolean }>;
  noPassword: boolean;
  hasApiToken: boolean;
  readOnly: boolean;
  isDemo: boolean;
  isAdmin: boolean;
  preferredName: string | null;
  displayName: string;
  phones: string[];
};

export async function settingsAccount(): Promise<SettingsAccount> {
  const profile = await loadProfile();
  return {
    id: profile.account_id,
    username: profile.username,
    emails: profile.emails.map((email, i) => ({ email, isPrimary: i === 0 })),
    noPassword: false,
    hasApiToken: false,
    readOnly: false,
    isDemo: profile.is_demo,
    isAdmin: profile.is_admin,
    preferredName: profile.preferred_name ?? null,
    displayName: ownerDisplayName(profile),
    phones: profile.phones,
  };
}

export async function listVaultImports(): Promise<VaultImportListItem[]> {
  const list = await vaultJson<Schemas["ImportsListResponse"]>("/v1/imports");
  return list.items.map((row) => ({
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
  const usage = await vaultJson<Schemas["AccountStorageResponse"]>(
    "/v1/account/storage",
  );
  return {
    totalBytes: usage.total_bytes,
    attachmentCount: usage.attachment_count,
    topAttachments: usage.top_attachments.map((row) => ({
      id: row.id,
      originalName: row.original_name ?? null,
      mimeType: row.mime_type ?? null,
      sizeBytes: row.size_bytes,
      conversationId: row.conversation_id,
      conversationTitle: row.conversation_title ?? null,
      chatIdentifier: row.chat_identifier,
    })),
  };
}
