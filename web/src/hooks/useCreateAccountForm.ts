import { useState } from "react";
import { ApiError } from "../lib/api";
import { createAccount } from "../lib/serverApi";
import type { components } from "../lib/serverApi.types";
import { useAsyncAction } from "./useAsyncAction";
import { useRouteMutation } from "./useRouteQuery";

/** What the server answers when it creates an account: the new account's row. */
type CreatedAccount = components["schemas"]["Account"];

/**
 * Creating an account: a username, the password twice, the checks, and the
 * request.
 *
 * The administrator creates accounts from the Account section of a new
 * account's Settings, opened from User Accounts. The server opens no session
 * for the new account, so what follows is `onCreated`'s: the administrator is
 * taken to the account's Settings.
 */
export function useCreateAccountForm({
  onCreated,
}: {
  /** Runs with the server's answer. The form stays busy until it settles, and shows what it throws. */
  onCreated: (created: CreatedAccount) => Promise<void> | void;
}) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const { busy, error, run } = useAsyncAction();
  // The new account joins User Accounts and the storage counts, so the
  // account's screens fetch again once the write settles.
  const create = useRouteMutation({
    mutationFn: (body: Parameters<typeof createAccount>[0]) => createAccount(body),
  });

  const submit = () => {
    if (busy) return;
    void run(async () => {
      if (!username.trim()) {
        throw new Error("Username is required.");
      }
      // Only the mismatch is checked here. Length is the server's rule, so it
      // stays there rather than being restated and left to drift.
      if (password !== confirmPassword) {
        throw new Error("Passwords do not match.");
      }
      let created: CreatedAccount;
      try {
        created = await create.mutateAsync({
          username: username.trim(),
          password,
          preferred_name: null,
          phone: null,
        });
      } catch (e: unknown) {
        // A taken username is the server's answer, worded for the log. The
        // form says the one thing the person can act on.
        if (e instanceof ApiError && e.type === "username-taken") {
          throw new Error("Invalid username.");
        }
        throw e;
      }
      await onCreated(created);
    });
  };

  return {
    username,
    setUsername,
    password,
    setPassword,
    confirmPassword,
    setConfirmPassword,
    busy,
    error,
    submit,
  };
}
