import { Link } from "react-router-dom";
import { useExternalStore } from "../lib/externalStore";
import { dismissNameRefusal, type NameRefusal, nameRefusals } from "../lib/nameRefusals";
import Button from "./Button";

/**
 * The whole store, as `useExternalStore`'s selector. It sits at module level
 * so it is the same function on every render.
 */
const everyRefusal = (refusals: readonly NameRefusal[]) => refusals;

/**
 * Says so across the top of the page when the server refused a display name
 * after the person had left the Settings screen it was typed on, so the typed
 * name is never dropped without a word (#2991). A refusal that arrives while
 * Settings is still open shows under the field instead. Each line links back
 * to the Profile the name came from, quotes the name so it can be typed again,
 * and stays until it is dismissed or a later save from that Profile succeeds.
 */
export default function NameRefusalNotice() {
  const refusals = useExternalStore(nameRefusals, everyRefusal);
  return refusals.map((refusal) => (
    <div
      key={refusal.profilePath}
      role="alert"
      className="flex shrink-0 items-center justify-center gap-3 border-b border-danger-soft-border bg-danger-soft-bg px-3 py-1.5 text-[0.813rem] text-danger"
    >
      <p className="m-0">
        {refusal.username === null ? "Your" : `${refusal.username}'s`} display name
        {refusal.name ? ` "${refusal.name}"` : null} wasn't saved: {refusal.reason}
      </p>
      <Link to={refusal.profilePath} className="text-danger underline">
        Go to Profile
      </Link>
      <Button
        variant="secondary"
        size="chip"
        onClick={() => dismissNameRefusal(refusal.profilePath)}
      >
        Dismiss
      </Button>
    </div>
  ));
}
