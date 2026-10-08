import { useState } from "react";
import { listedServerService } from "../lib/offeredService";
import type { components } from "../lib/serverApi.types";

/** The pick the server takes, as `set_identity_country` in a contact or account update. */
export type IdentityCountryPick = components["schemas"]["SetIdentityCountryRequest"];

/** An identity as an identity table lists it. */
export interface IdentityCountryTarget {
  address: string;
  service: string | null;
}

/**
 * The state of the Pick country dialog (`IdentityCountryDialog`) for one
 * identity table: which identity it is open for, and why the last pick failed
 * (#1676). The contact drawer and My Identities both use it, each with the
 * update that carries the pick.
 *
 * `send` sends the pick and settles when the server has answered. The dialog
 * closes when it succeeds and stays open on a refusal, which it shows, so a
 * merge question can be answered and a failure retried.
 */
export function useIdentityCountryPick(send: (pick: IdentityCountryPick) => Promise<unknown>) {
  const [target, setTarget] = useState<IdentityCountryTarget | null>(null);
  const [error, setError] = useState<Error | null>(null);

  /** Open the dialog for `next`. */
  const request = (next: IdentityCountryTarget) => {
    setError(null);
    setTarget(next);
  };

  /** Close the dialog. */
  const close = () => setTarget(null);

  /**
   * Pick the country of the number written without its `+` code. With
   * `merge`, it joins the identity the server named as holding its `+` form.
   */
  const confirm = async ({ country, merge }: { country: string; merge: boolean }) => {
    if (!target) return;
    setError(null);
    try {
      await send({
        address: target.address,
        service: listedServerService(target.service),
        country,
        merge,
      });
      setTarget(null);
    } catch (e) {
      setError(e instanceof Error ? e : new Error(String(e)));
    }
  };

  return { target, error, request, close, confirm };
}
