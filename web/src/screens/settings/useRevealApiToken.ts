import { useContext } from "react";
import { type ApiTokenReveal, ApiTokenRevealContext } from "../../components/apiTokenRevealState";

/** Opens the reveal dialog of the nearest `ApiTokenRevealProvider`. */
export function useRevealApiToken(): (reveal: ApiTokenReveal) => void {
  const reveal = useContext(ApiTokenRevealContext);
  if (reveal === null) throw new Error("useRevealApiToken needs an ApiTokenRevealProvider");
  return reveal;
}
