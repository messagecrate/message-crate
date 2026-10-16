import { type ReactNode, useState } from "react";
import AuthSubmitButton from "../../components/AuthSubmitButton";
import Checkbox from "../../components/Checkbox";
import type { LoginLifetime } from "../../lib/authContext";
import { isTauri } from "../../lib/tauriCheck";

/** What "Keep me logged in" says, and whether the card shows it at all. */
export type KeepLoggedInChoice = {
  /** False in the desktop app, which ends the Session when its window closes. */
  offered: boolean;
  keep: boolean;
  setKeep: (keep: boolean) => void;
  /** How long the saved login lasts: kept past this visit, or for this visit only. */
  lifetime: LoginLifetime;
};

/**
 * "Keep me logged in" for a form on the login card, ticked to start with.
 * Unticked, the saved login is kept for this visit only, for a shared or
 * borrowed computer (#2205). The desktop app does not offer it and always
 * keeps the login.
 */
export function useKeepLoggedIn(): KeepLoggedInChoice {
  const [keep, setKeep] = useState(true);
  const offered = !isTauri();
  return { offered, keep, setKeep, lifetime: !offered || keep ? "kept" : "visit" };
}

/**
 * A login card form's submit button, with "Keep me logged in" beside it on
 * the website. The desktop app shows the button on its own row.
 */
export function KeepLoggedInSubmit({
  choice,
  disabled,
  children,
}: {
  choice: KeepLoggedInChoice;
  disabled: boolean;
  children: ReactNode;
}) {
  if (!choice.offered) {
    return <AuthSubmitButton disabled={disabled}>{children}</AuthSubmitButton>;
  }
  return (
    <div className="mt-5 flex items-center justify-between gap-3">
      <Checkbox checked={choice.keep} onChange={choice.setKeep} disabled={disabled}>
        Keep me logged in
      </Checkbox>
      <AuthSubmitButton disabled={disabled} className="min-w-[50%]">
        {children}
      </AuthSubmitButton>
    </div>
  );
}
