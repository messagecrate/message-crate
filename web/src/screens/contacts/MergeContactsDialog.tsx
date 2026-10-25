import { useState } from "react";
import { ListBoxItem } from "react-aria-components";
import Button from "../../components/Button";
import ContactLabel from "../../components/ContactLabel";
import ModalShell, { DialogError, DialogFooter } from "../../components/ModalShell";
import Select, { selectItemClassName } from "../../components/Select";
import { useResetOnChange } from "../../hooks/useResetOnChange";
import { useRouteQuery } from "../../hooks/useRouteQuery";
import { errorLine } from "../../lib/apiErrorMessage";
import { countOf } from "../../lib/plural";
import { keys } from "../../lib/queryKeys";
import { getContact } from "../../lib/serverApi";
import type { components } from "../../lib/serverApi.types";
import { serviceLabel } from "../../lib/serviceLabel";

type ContactDetail = components["schemas"]["Contact"];

/**
 * Most contacts one merge takes, as the server holds it
 * (`MAX_MERGED_CONTACTS` in `crates/server/server/src/db/contact_merges.rs`).
 */
export const MAX_MERGED_CONTACTS = 10;

/** A checked row of the Contacts list, as the dialog takes it. */
export type MergeCandidate = { id: number; name: string; addresses: string[] };

/** The screen name of each identity type the server sends. */
const TYPE_LABELS: Record<string, string> = {
  phone: "Phone",
  email: "Email",
  username: "Username",
  other: "Other",
};

const headClass = "text-[0.688rem] uppercase tracking-[0.05em] text-muted";
const cellClass = "truncate py-0.5 pr-3.5 align-baseline";

/**
 * The contact a merge keeps when no name can be chosen: the one with the
 * most messages, the first of them on a tie.
 */
function busiest(details: readonly ContactDetail[]): number | null {
  let best: ContactDetail | null = null;
  for (const d of details) if (!best || d.total_messages > best.total_messages) best = d;
  return best?.id ?? null;
}

/**
 * Merge Contacts' confirmation (#3237): the name the merged contact keeps,
 * and every identity that moves to it. The contact that owns the chosen name
 * is the one the others merge into. A contact with no name is listed by its
 * first identity, in italics as on the Contacts list, and cannot be chosen.
 * With one named contact its name is chosen already; with none, the dropdown
 * reads "(no name)" and the contact with the most messages is kept.
 */
export default function MergeContactsDialog({
  open,
  contacts,
  busy,
  onMerge,
  onClose,
}: {
  open: boolean;
  /** The checked contacts, two to ten. */
  contacts: readonly MergeCandidate[];
  /** A merge, an undo or a redo is sending its requests. */
  busy: boolean;
  /** Merge the others into `targetId`; answers what refused it, or null when it went through. */
  onMerge: (targetId: number) => Promise<unknown>;
  onClose: () => void;
}) {
  const ids = contacts.map((c) => c.id);
  // Set while the merge is sent: the merge marks every contact stale, and
  // the contacts it took are gone, so they are not read again.
  const [sending, setSending] = useState(false);
  const details = useRouteQuery(
    keys.contacts.merging(ids.map(String)),
    (signal) => Promise.all(ids.map((id) => getContact(id, { signal }))),
    { enabled: open && ids.length > 0 && !sending },
  );
  const named = contacts.filter((c) => c.name.trim());
  const [chosen, setChosen] = useState<number | null>(null);
  const [error, setError] = useState("");
  useResetOnChange([open, ids.join(",")], () => {
    setChosen(null);
    setError("");
    setSending(false);
  });

  const loaded = details.data ?? null;
  const byId = new Map((loaded ?? []).map((d) => [d.id, d]));
  const onlyNamed = named.length === 1 ? (named[0]?.id ?? null) : null;
  const target = named.length === 0 ? (loaded ? busiest(loaded) : null) : (chosen ?? onlyNamed);
  const title = `Merge ${contacts.length} contacts`;

  // The kept contact's identities first, then each other's in list order.
  const ordered = [...contacts].sort((a, b) => Number(b.id === target) - Number(a.id === target));
  const rows = ordered.flatMap((c) =>
    (byId.get(c.id)?.identities ?? []).map((identity) => ({ contact: c, identity })),
  );

  const merge = async () => {
    if (target === null) return;
    setError("");
    setSending(true);
    const refusal = await onMerge(target);
    if (refusal) {
      setSending(false);
      setError(errorLine(refusal, "Could not merge the contacts."));
    }
  };

  return (
    <ModalShell
      open={open}
      onOpenChange={(o) => {
        if (!o && !busy) onClose();
      }}
      dismissable={!busy}
      label={title}
      title={title}
      onClose={onClose}
      closeDisabled={busy}
      maxWidth="34rem"
    >
      <p className="mb-4 text-[0.875rem] leading-relaxed text-muted">
        Choose the name the merged contact keeps.
      </p>
      <Select
        label="Name to keep"
        aria-label="Name to keep"
        placeholder={named.length === 0 ? "(no name)" : "Choose a name"}
        selectedKey={named.length === 0 ? null : target}
        disabledKeys={contacts.filter((c) => !c.name.trim()).map((c) => c.id)}
        isDisabled={named.length === 0 || busy}
        // The closed dropdown shows the name alone; the count is for the list.
        valueClassName="[&_[data-count]]:hidden"
        onSelectionChange={(key) => setChosen(typeof key === "number" ? key : Number(key))}
      >
        {contacts.map((c) => {
          const count = byId.get(c.id)?.total_messages;
          const nameless = !c.name.trim();
          return (
            <ListBoxItem
              key={c.id}
              id={c.id}
              textValue={c.name.trim() || c.addresses[0] || ""}
              className={(state) =>
                `${selectItemClassName(state)} flex justify-between gap-3 ${
                  nameless ? "cursor-default opacity-50" : ""
                }`
              }
            >
              <span className="min-w-0 truncate">
                <ContactLabel name={c.name} addresses={c.addresses} />
              </span>
              {count !== undefined ? (
                <span data-count="" className="shrink-0 text-muted">
                  {countOf(count, "message")}
                </span>
              ) : null}
            </ListBoxItem>
          );
        })}
      </Select>

      <section className="mt-5 border-t border-border pt-4" aria-label="Merged identities">
        <h3 className="m-0 text-[0.875rem] font-semibold text-text">Merged identities</h3>
        <p className="mt-0.5 mb-3 text-[0.75rem] text-muted">
          All of these move to the contact you keep.
        </p>
        {details.error ? (
          <DialogError message={errorLine(details.error, "Could not load the identities.")} />
        ) : !loaded ? (
          <p className="text-[0.813rem] text-muted">Loading…</p>
        ) : (
          <div className="max-h-60 overflow-auto">
            <table className="w-full table-fixed border-collapse text-left text-[0.813rem]">
              <thead>
                <tr>
                  {["From", "Identity", "Type", "Service"].map((h) => (
                    <th key={h} className={`pb-1.5 pr-3.5 font-semibold ${headClass}`}>
                      {h}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {rows.map(({ contact, identity }) => (
                  <tr key={`${contact.id}-${identity.service}-${identity.address}`}>
                    <td className={`${cellClass} text-text`}>
                      <ContactLabel name={contact.name} addresses={contact.addresses} />
                    </td>
                    <td className={`${cellClass} text-text`} title={identity.address}>
                      {identity.address}
                    </td>
                    <td className={`${cellClass} text-muted`}>
                      {TYPE_LABELS[identity.identity_type] ?? identity.identity_type}
                    </td>
                    <td className={`${cellClass} text-muted`}>
                      {serviceLabel(identity.service) ?? "—"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <DialogError message={error} />
      <DialogFooter>
        <Button onPress={onClose} isDisabled={busy}>
          Cancel
        </Button>
        <Button variant="primary" onPress={() => void merge()} isDisabled={busy || target === null}>
          {busy ? "Merging…" : "Merge"}
        </Button>
      </DialogFooter>
    </ModalShell>
  );
}
