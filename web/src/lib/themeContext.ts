/**
 * The theme, as `ThemeProvider` fills it and `useTheme`
 * (`components/useTheme.ts`) reads it.
 */
import { createContext } from "react";
import type { ResolvedTheme, ThemeMode, ThemePreset, ThemeSeeds } from "./theme";

export type UseThemeResult = {
  mode: ThemeMode;
  setMode: (mode: ThemeMode) => void;
  seeds: ThemeSeeds;
  setSeeds: (seeds: ThemeSeeds) => void;
  patchSeed: (key: keyof ThemeSeeds, hex: string) => void;
  shareString: string;
  setShareString: (raw: string) => boolean;
  applyPreset: (preset: ThemePreset) => void;
  resolvedMode: ResolvedTheme;
  presets: ThemePreset[];
};

export const ThemeContext = createContext<UseThemeResult | null>(null);
