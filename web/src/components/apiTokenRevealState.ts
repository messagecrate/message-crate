import { createContext } from "react";

export type ApiTokenReveal = { label: string; token: string };

/** Set by `ApiTokenRevealProvider`; opens its reveal dialog. */
export const ApiTokenRevealContext = createContext<((reveal: ApiTokenReveal) => void) | null>(null);
