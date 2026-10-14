import { type Dispatch, type RefObject, type SetStateAction, useRef, useState } from "react";

/**
 * A display name typed on the Profile tab and not yet saved, with the
 * server's refusal of it. The Settings screen holds it, not the panel: a tab
 * switch removes the panel while its save is still on the way, and a refusal
 * that arrives then is shown when the person comes back to Profile, with the
 * name they typed still in the field (#2985).
 */
export interface NameEdit {
  /** What the person has typed and not yet saved. Null shows the stored name. */
  draft: string | null;
  setDraft: Dispatch<SetStateAction<string | null>>;
  error: string;
  setError: Dispatch<SetStateAction<string>>;
  /** The name a save has sent and not yet heard back about, so it is not sent twice. */
  inFlight: RefObject<string | null>;
}

export function useNameEdit(): NameEdit {
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState("");
  const inFlight = useRef<string | null>(null);
  return { draft, setDraft, error, setError, inFlight };
}
