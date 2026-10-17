import { useContext } from "react";
import { ThemeContext, type UseThemeResult } from "../lib/themeContext";

/** Current theme. Must be called under ThemeProvider. */
export function useTheme(): UseThemeResult {
  const ctx = useContext(ThemeContext);
  if (!ctx) {
    throw new Error("useTheme must be used within ThemeProvider");
  }
  return ctx;
}
