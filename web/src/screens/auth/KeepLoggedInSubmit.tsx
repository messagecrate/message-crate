import type { ReactNode } from "react";
import AuthSubmitButton from "../../components/AuthSubmitButton";
import Checkbox from "../../components/Checkbox";
import type { KeepLoggedInChoice } from "./useKeepLoggedIn";

/**
 * A login card form's submit button, with "Keep me logged in" beside it on
 * the website. The desktop app shows the button on its own row.
 */
export default function KeepLoggedInSubmit({
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
