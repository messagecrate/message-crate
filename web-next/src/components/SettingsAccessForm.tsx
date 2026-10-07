"use client";

import { useCallback, useEffect, useState } from "react";
import { ApiTokenRevealDialog } from "./ApiTokenRevealDialog";

type AccessData = {
  readOnly: boolean;
  hasApiToken: boolean;
  username: string;
};

export function SettingsAccessForm() {
  const [readOnly, setReadOnly] = useState(false);
  const [hasApiToken, setHasApiToken] = useState(false);
  const [username, setUsername] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [tokenBusy, setTokenBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [revealedToken, setRevealedToken] = useState<string | null>(null);

  const apply = (json: AccessData) => {
    setReadOnly(json.readOnly);
    setHasApiToken(json.hasApiToken);
    setUsername(json.username ?? "");
  };

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch("/api/settings/account");
      const json = (await res.json()) as AccessData & { error?: string };
      if (!res.ok) throw new Error(json.error ?? "Couldn’t load access settings.");
      apply(json);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Couldn’t load access settings.");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const saveReadOnly = async (nextReadOnly: boolean) => {
    setReadOnly(nextReadOnly);
    setSaving(true);
    setError(null);
    try {
      const res = await fetch("/api/settings/account", {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ readOnly: nextReadOnly }),
      });
      const json = (await res.json()) as AccessData & { error?: string };
      if (!res.ok) throw new Error(json.error ?? "Couldn’t save your changes.");
      apply(json);
    } catch (err) {
      setReadOnly(!nextReadOnly);
      setError(err instanceof Error ? err.message : "Couldn’t save your changes.");
    } finally {
      setSaving(false);
    }
  };

  const generateApiToken = async () => {
    setTokenBusy(true);
    setError(null);
    try {
      const res = await fetch("/api/settings/account", {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ generateApiToken: true }),
      });
      const json = (await res.json()) as AccessData & {
        token?: string;
        error?: string;
      };
      if (!res.ok) throw new Error(json.error ?? "Couldn’t create an API token.");
      apply(json);
      if (json.token) setRevealedToken(json.token);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Couldn’t create an API token.",
      );
    } finally {
      setTokenBusy(false);
    }
  };

  const deleteApiToken = async () => {
    if (
      !window.confirm(
        "Delete this API token? You’ll need a new one to import messages again.",
      )
    ) {
      return;
    }
    setTokenBusy(true);
    setError(null);
    try {
      const res = await fetch("/api/settings/account", {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ deleteApiToken: true }),
      });
      const json = (await res.json()) as AccessData & { error?: string };
      if (!res.ok) throw new Error(json.error ?? "Couldn’t delete the API token.");
      apply(json);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Couldn’t delete the API token.",
      );
    } finally {
      setTokenBusy(false);
    }
  };

  if (loading) {
    return <p className="text-[14px] text-muted">Loading…</p>;
  }

  return (
    <div className="max-w-xl space-y-10">
      <section>
        <h2 className="text-[12px] font-semibold tracking-wider text-muted uppercase">
          Browsing access
        </h2>
        <p className="mt-1 text-[13px] text-muted">
          Choose whether messages and contacts can be changed.
        </p>
        <label className="mt-4 flex items-start gap-2.5">
          <input
            type="checkbox"
            checked={readOnly}
            disabled={saving || tokenBusy}
            onChange={(e) => void saveReadOnly(e.target.checked)}
            className="mt-0.5 size-4 rounded border-border accent-accent"
          />
          <span>
            <span className="block text-[13px] text-text">View-only mode</span>
            <span className="block text-[12px] text-muted">
              Block edits and deletions while browsing. Settings and imports
              remain available.
            </span>
          </span>
        </label>
      </section>

      <section>
        <h2 className="text-[12px] font-semibold tracking-wider text-muted uppercase">
          Message import
        </h2>
        <p className="mt-1 text-[13px] text-muted">
          Create an API token to import messages. It is shown only once. In Message
          Exporters → Vault, paste the token and set Vault URL to your vault origin
          (for example <span className="font-mono text-text">http://127.0.0.1:8080</span>{" "}
          or <span className="font-mono text-text">https://app.bitrealm.io</span>
          ).
          {username ? (
            <>
              {" "}
              Your User ID is{" "}
              <span className="font-mono text-text">{username}</span> (shown for
              reference; the token alone identifies the account).
            </>
          ) : null}
        </p>
        <div className="mt-4">
          <h3 className="text-[13px] font-medium text-text">API token</h3>
          <div className="mt-2 flex flex-col gap-2 sm:flex-row sm:items-center">
            <input
              type="text"
              readOnly
              tabIndex={-1}
              onMouseDown={(event) => event.preventDefault()}
              onCopy={(event) => event.preventDefault()}
              onContextMenu={(event) => event.preventDefault()}
              value={
                hasApiToken ? "mv-user-••••••••••••••••••••••••••••••••" : ""
              }
              aria-label={
                hasApiToken ? "API token is set" : "No API token"
              }
              placeholder="No API token"
              className="pointer-events-none min-w-0 flex-1 select-none rounded-md border border-border bg-elevated px-3 py-2 font-mono text-[12px] text-text outline-none placeholder:text-muted"
            />
            {hasApiToken ? (
              <button
                type="button"
                disabled={tokenBusy || saving}
                onClick={() => void deleteApiToken()}
                className="shrink-0 rounded-md border border-red-500/40 bg-red-500/15 px-3 py-2 text-[13px] text-red-100 transition-colors hover:bg-red-500/25 disabled:opacity-50"
              >
                {tokenBusy ? "…" : "Delete"}
              </button>
            ) : (
              <button
                type="button"
                disabled={tokenBusy || saving}
                onClick={() => void generateApiToken()}
                className="shrink-0 rounded-md border border-border bg-elevated px-3 py-2 text-[13px] text-text transition-colors hover:bg-hover disabled:opacity-50"
              >
                {tokenBusy ? "…" : "Generate"}
              </button>
            )}
          </div>
        </div>
      </section>

      {error && (
        <p className="text-[13px] text-danger" role="alert">
          {error}
        </p>
      )}

      <ApiTokenRevealDialog
        open={revealedToken != null}
        token={revealedToken ?? ""}
        onClose={() => setRevealedToken(null)}
      />
    </div>
  );
}
