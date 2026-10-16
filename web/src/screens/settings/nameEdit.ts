import {
  type Dispatch,
  type SetStateAction,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { getAccountId } from "../../lib/api";
import { errorLine } from "../../lib/apiErrorMessage";
import { reportNameRefusal } from "../../lib/nameRefusals";

/**
 * A display name typed on the Profile tab and not yet saved, with the
 * server's refusal of it. The Settings screen holds it, not the panel: a tab
 * switch removes the panel while its save is still on the way, and a refusal
 * that arrives then is shown when the person comes back to Profile, with the
 * name they typed still in the field (#2985). A refusal that arrives after
 * the Settings screen itself has gone is shown across the top of the page
 * instead (#2991).
 */
export interface NameEdit {
  /** What the person has typed and not yet saved. Null shows the stored name. */
  draft: string | null;
  setDraft: Dispatch<SetStateAction<string | null>>;
  error: string;
  /**
   * Save the draft, given the stored name and the call that sends a name
   * (`null` clears it). A draft that reads as the stored name sends nothing.
   */
  save: (stored: string | null, send: (name: string | null) => Promise<unknown>) => Promise<void>;
}

/** Where a name was typed, for a refusal that arrives once the screen has gone. */
export interface NameEditOrigin {
  /** The Profile tab the name is typed on. */
  profilePath: string;
  /** The account the name is for when the owner opened it; null for the logged-in account. */
  username: string | null;
}

export function useNameEdit(origin: NameEditOrigin): NameEdit {
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState("");
  // Whether the screen that holds this edit is still there to show a refusal.
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const originRef = useRef(origin);
  useEffect(() => {
    originRef.current = origin;
  });
  // The name the newest save sent, until its answer arrives. Leaving the field
  // and then the panel does not send it twice, and the answer to an older
  // save, which a newer one has overtaken, changes nothing.
  const sending = useRef<string | null>(null);

  const save = useCallback(
    async (stored: string | null, send: (name: string | null) => Promise<unknown>) => {
      if (draft === null) return;
      const next = draft.trim();
      if (next === (stored ?? "")) {
        setDraft(null);
        setError("");
        return;
      }
      if (sending.current === next) return;
      sending.current = next;
      setError("");
      const accountId = getAccountId();
      try {
        await send(next || null);
        if (sending.current !== next) return;
        // Only the name that was sent is cleared. A name typed while it was
        // on the way stays, to be saved in its turn.
        setDraft((d) => (d !== null && d.trim() === next ? null : d));
      } catch (e) {
        if (sending.current !== next) return;
        if (mounted.current) setError(errorLine(e));
        else reportNameRefusal({ accountId, ...originRef.current, reason: errorLine(e) });
      } finally {
        if (sending.current === next) sending.current = null;
      }
    },
    [draft],
  );

  return { draft, setDraft, error, save };
}
