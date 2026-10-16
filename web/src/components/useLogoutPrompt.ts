import { useContext } from "react";
import { type LogoutPrompt, LogoutPromptContext } from "../lib/logoutPromptContext";

/** What logging out shows. Must be called under AuthProvider. */
export function useLogoutPrompt(): LogoutPrompt {
  const ctx = useContext(LogoutPromptContext);
  if (!ctx) throw new Error("useLogoutPrompt must be used within AuthProvider");
  return ctx;
}
