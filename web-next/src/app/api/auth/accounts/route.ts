import { createAccount } from "@/lib/accounts";
import { accountCookieOptions } from "@/lib/session";
import { MAX_PASSWORD_LENGTH, validatePasswordPlaintext } from "@/lib/password";
import { cookies } from "next/headers";
import { NextResponse } from "next/server";
import { writesAvailable, writesNotAvailable } from "@/lib/vault/writes";

export const runtime = "nodejs";

export async function GET(req: Request) {
  try {
    const url = new URL(req.url);
    const check = url.searchParams.get("username");
    if (check != null) {
      const username = check.trim();
      if (!username) {
        return NextResponse.json({ taken: false });
      }
      // The vault answers 409 on register for a taken name; there is no
      // check route, so the form learns on submit.
      return NextResponse.json({ taken: false, username });
    }
    // The vault has no account list for signed-out callers.
    return NextResponse.json({ accounts: [] });
  } catch (err) {
    const message = err instanceof Error ? err.message : "failed to list accounts";
    return NextResponse.json({ error: message }, { status: 500 });
  }
}

export async function POST(req: Request) {
  if (!writesAvailable()) return writesNotAvailable();

  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid JSON" }, { status: 400 });
  }

  const username = typeof body.username === "string" ? body.username.trim() : "";
  const preferredName =
    typeof body.preferredName === "string"
      ? body.preferredName.trim()
      : typeof body.displayName === "string"
        ? body.displayName.trim()
        : typeof body.firstName === "string" || typeof body.lastName === "string"
          ? [body.firstName, body.lastName]
              .filter((p): p is string => typeof p === "string")
              .map((p) => p.trim())
              .filter(Boolean)
              .join(" ")
          : "";
  const phone = typeof body.phone === "string" ? body.phone.trim() : "";
  const noPassword = body.noPassword === true;
  const password = typeof body.password === "string" ? body.password : "";

  if (!username || !preferredName || !phone) {
    return NextResponse.json(
      { error: "username, preferredName, and phone are required" },
      { status: 400 },
    );
  }

  if (!noPassword) {
    const pwdErr = validatePasswordPlaintext(password);
    if (pwdErr) {
      return NextResponse.json({ error: pwdErr }, { status: 400 });
    }
    if (password.length >= MAX_PASSWORD_LENGTH) {
      return NextResponse.json(
        { error: "password must be less than 100 characters" },
        { status: 400 },
      );
    }
  }

  try {
    const account = await createAccount({
      username,
      preferredName,
      phone,
      password: noPassword ? null : password,
      noPassword,
    });
    const store = await cookies();
    store.set(accountCookieOptions(account.id));
    return NextResponse.json({
      id: account.id,
      username: account.username,
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : "create failed";
    const status =
      message.includes("already taken") ||
      message.includes("already used") ||
      message.includes("E.164")
        ? 409
        : message.includes("required") ||
            message.includes("valid phone") ||
            message.includes("password")
          ? 400
          : 500;
    return NextResponse.json({ error: message }, { status });
  }
}
