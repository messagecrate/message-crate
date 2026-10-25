import assert from "node:assert/strict";
import { afterEach, describe, it } from "node:test";

import { NOT_SIGNED_IN } from "./accountContext";
import { redirectWhenSignedOut } from "./serverAccount";
import { SESSION_ENDED_PATH } from "./sessionEnded";
import { VaultError, vaultJson } from "./vault/client";

/** The URL a `redirect()` error sends the browser to. */
function redirectTarget(err: unknown): string | null {
  const digest = (err as { digest?: unknown })?.digest;
  if (typeof digest !== "string" || !digest.startsWith("NEXT_REDIRECT;")) {
    return null;
  }
  return digest.split(";").slice(2, -2).join(";");
}

describe("redirectWhenSignedOut", () => {
  const realFetch = globalThis.fetch;
  afterEach(() => {
    globalThis.fetch = realFetch;
  });

  it("sends a page whose read the server refused to the session-ended route", async () => {
    globalThis.fetch = async () => new Response(null, { status: 401 });
    const err = await redirectWhenSignedOut(() =>
      vaultJson("/v1/session", { token: "mc-user-stale" }),
    ).catch((e: unknown) => e);
    assert.equal(redirectTarget(err), SESSION_ENDED_PATH);
  });

  it("sends a page with no session cookie to the session-ended route", async () => {
    const err = await redirectWhenSignedOut(async () => {
      throw new Error(NOT_SIGNED_IN);
    }).catch((e: unknown) => e);
    assert.equal(redirectTarget(err), SESSION_ENDED_PATH);
  });

  it("throws any other failure as it is", async () => {
    const failure = new VaultError(500, "database is locked");
    const err = await redirectWhenSignedOut(async () => {
      throw failure;
    }).catch((e: unknown) => e);
    assert.equal(err, failure);
  });

  it("answers what the read answers", async () => {
    assert.equal(await redirectWhenSignedOut(async () => 7), 7);
  });
});
