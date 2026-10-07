import {
  accountHasApiToken,
  accountHasNoPassword,
  deleteAccount,
  deleteAccountApiToken,
  loadAccount,
  rotateAccountApiToken,
  saveAccount,
  setAccountPassword,
  type AccountEmail,
} from "@/lib/accounts";
import {
  unauthorizedResponse,
  withAccountHandler,
} from "@/lib/accountContext";
import {
  loadAccountProfile,
  saveAccountProfile,
} from "@/lib/accountProfile";
import { isDemoAccount } from "@/lib/demoAccount";
import { validatePasswordPlaintext } from "@/lib/password";
import { clearAccountCookieOptions } from "@/lib/session";
import { settingsAccount } from "@/lib/vault/account";
import { cookies } from "next/headers";
import { NextResponse } from "next/server";
import { writesAvailable, writesNotAvailable } from "@/lib/vault/writes";

export const runtime = "nodejs";

function accountJson(account: ReturnType<typeof loadAccount>, accountId: string) {
  const profile = loadAccountProfile(accountId);
  return {
    id: account.id,
    username: account.username,
    emails: account.emails.map((entry) => ({ email: entry.email })),
    noPassword: accountHasNoPassword(accountId),
    hasApiToken: accountHasApiToken(accountId),
    isDemo: isDemoAccount(accountId),
    preferredName: profile.preferred_name,
    displayName: profile.display_name,
    phones: profile.phones,
  };
}

function parseEmails(body: Record<string, unknown>): AccountEmail[] | undefined {
  if (!Array.isArray(body.emails)) return undefined;

  const emails: AccountEmail[] = [];
  for (const item of body.emails) {
    if (!item || typeof item !== "object") continue;
    const row = item as Record<string, unknown>;
    if (typeof row.email !== "string" || !row.email.trim()) continue;
    emails.push({ email: row.email.trim() });
  }
  return emails;
}

function authError(err: unknown): NextResponse | null {
  if (err instanceof Error && err.message === "Not signed in") {
    return unauthorizedResponse();
  }
  return null;
}

export async function GET() {
  try {
    return await withAccountHandler(async () => {
      return NextResponse.json(await settingsAccount());
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    return NextResponse.json(
      { error: "Couldn’t load your account." },
      { status: 500 },
    );
  }
}

export async function PATCH(req: Request) {
  if (!writesAvailable()) return writesNotAvailable();
  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json(
      { error: "Couldn’t save your changes." },
      { status: 400 },
    );
  }

  try {
    return await withAccountHandler(async (accountId) => {
      const generateApiToken = body.generateApiToken === true;
      const deleteApiToken = body.deleteApiToken === true;
      const clearPassword = body.noPassword === true;
      const password =
        typeof body.password === "string" ? body.password : undefined;
      if (clearPassword && password !== undefined) {
        return NextResponse.json(
          { error: "Choose either a password or passwordless sign-in." },
          { status: 400 },
        );
      }
      if (password !== undefined) {
        const passwordError = validatePasswordPlaintext(password);
        if (passwordError) {
          return NextResponse.json({ error: passwordError }, { status: 400 });
        }
      }

      if (generateApiToken) {
        const token = rotateAccountApiToken(accountId);
        const account = loadAccount(accountId);
        return NextResponse.json({
          ...accountJson(account, accountId),
          token,
        });
      }

      if (deleteApiToken) {
        deleteAccountApiToken(accountId);
        const account = loadAccount(accountId);
        return NextResponse.json(accountJson(account, accountId));
      }

      const patch: {
        username?: string;
        emails?: AccountEmail[];
      } = {};

      if (typeof body.username === "string" && body.username.trim()) {
        patch.username = body.username.trim();
      }

      const emails = parseEmails(body);
      if (emails !== undefined) {
        patch.emails = emails;
      }

      const hasIdentityPatch =
        typeof body.preferredName === "string" ||
        typeof body.displayName === "string" ||
        Array.isArray(body.phones);

      if (
        patch.username === undefined &&
        patch.emails === undefined &&
        !hasIdentityPatch &&
        !clearPassword &&
        password === undefined
      ) {
        return NextResponse.json({ error: "Nothing to save." }, { status: 400 });
      }

      const account =
        patch.username !== undefined || patch.emails !== undefined
          ? saveAccount(accountId, patch)
          : loadAccount(accountId);

      if (hasIdentityPatch) {
        const current = loadAccountProfile(accountId);
        const phones = Array.isArray(body.phones)
          ? body.phones
              .filter((p): p is string => typeof p === "string")
              .map((p) => p.trim())
              .filter(Boolean)
          : current.phones;
        if (phones.length === 0) {
          return NextResponse.json(
            { error: "At least one phone number is required." },
            { status: 400 },
          );
        }
        const preferredName =
          typeof body.preferredName === "string"
            ? body.preferredName
            : typeof body.displayName === "string"
              ? body.displayName
              : current.preferred_name ?? "";
        if (!preferredName.trim()) {
          return NextResponse.json(
            { error: "Display name is required." },
            { status: 400 },
          );
        }
        saveAccountProfile(accountId, {
          preferred_name: preferredName,
          phones,
        });
      }

      if (clearPassword || password !== undefined) {
        await setAccountPassword(accountId, clearPassword ? null : password!);
      }

      return NextResponse.json(accountJson(account, accountId));
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    const message =
      err instanceof Error ? err.message : "Couldn’t save your changes.";
    const phoneValidationError = message.startsWith(
      "Enter a valid phone number",
    );
    const userMessage =
      body.generateApiToken === true
        ? "Couldn’t create an API token."
        : body.deleteApiToken === true
          ? "Couldn’t delete the API token."
          : phoneValidationError
            ? message
            : "Couldn’t save your changes.";
    return NextResponse.json(
      { error: userMessage },
      { status: phoneValidationError ? 400 : 500 },
    );
  }
}

export async function DELETE() {
  if (!writesAvailable()) return writesNotAvailable();
  try {
    return await withAccountHandler(async (accountId) => {
      deleteAccount(accountId);
      const store = await cookies();
      store.set(clearAccountCookieOptions());
      return NextResponse.json({ ok: true });
    });
  } catch (err) {
    const auth = authError(err);
    if (auth) return auth;
    return NextResponse.json(
      { error: "Couldn’t delete your account." },
      { status: 500 },
    );
  }
}
