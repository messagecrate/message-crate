import { type KeyboardEvent, useId, useRef, useState } from "react";
import Button from "../../components/Button";
import { XIcon } from "../../components/icons";
import ModalShell, { DialogError, DialogFooter } from "../../components/ModalShell";
import PlainButton from "../../components/PlainButton";
import Select, { ListBoxItem, selectItemClassName } from "../../components/Select";
import { useRouteMutation } from "../../hooks/useRouteQuery";
import { ApiError } from "../../lib/api";
import { errorLine } from "../../lib/apiErrorMessage";
import {
  identityDuplicateKey,
  identityPlaceholder,
  identityValidationError,
  OFFERED_SERVICE_OPTIONS,
  OFFERED_SERVICES,
  type OfferedService,
  serverService,
} from "../../lib/offeredService";
import { parseSelectKey } from "../../lib/selectKey";
import { createContact } from "../../lib/serverApi";
import type { components } from "../../lib/serverApi.types";
import { focusRingClass } from "../../lib/uiStyles";
import { Z_POPOVER_IN_MODAL } from "../../lib/zLayers";

type Contact = components["schemas"]["Contact"];

const fieldLabelClass = "mb-1 block text-[0.813rem] font-medium text-text";
const inputClass =
  "box-border w-full rounded border border-border bg-elevated px-3 py-2 text-[0.875rem] font-normal leading-none text-text outline-none focus:border-accent";
const selectTriggerClass =
  "!box-border !h-9 !min-h-9 !w-full !rounded !px-3 !py-0 !text-[0.875rem] !font-normal !leading-none !bg-elevated";
const selectValueClass = "!text-[0.875rem] !font-normal !leading-none";

const DUPLICATE_MESSAGE = "This identity is already in the list.";

/** One identity row of the form. `key` keeps a row's place while rows above it go. */
type IdentityRow = { key: number; service: OfferedService; address: string };

/** The server names an identity it refused by its place in the request: `identities[2].address: …`. */
const IDENTITY_ERROR = /^identities\[(\d+)\]\.address: (.*)$/;

/** `text` with its first letter capitalized and a final period, as a line under a field. */
function asLine(text: string): string {
  const capitalized = text.charAt(0).toUpperCase() + text.slice(1);
  return /[.!?]$/.test(capitalized) ? capitalized : `${capitalized}.`;
}

/**
 * The server's refusal split by where it belongs: a sentence under each
 * identity row it names, by the row's key, and the rest for the line under
 * the form. `sent` holds the keys of the rows the request carried, in order.
 */
function placeRefusal(
  err: unknown,
  sent: readonly number[],
): { rows: Map<number, string>; rest: string } {
  const rows = new Map<number, string>();
  if (!(err instanceof ApiError) || err.errors.length === 0) {
    return { rows, rest: errorLine(err, "Could not add the contact.") };
  }
  const rest: string[] = [];
  for (const sentence of err.errors) {
    const found = IDENTITY_ERROR.exec(sentence);
    const key = found ? sent[Number(found[1])] : undefined;
    if (found && key !== undefined) rows.set(key, asLine(found[2] ?? ""));
    else rest.push(sentence);
  }
  return { rows, rest: rest.length > 0 ? errorLine(new ApiError(422, rest.join("; "))) : "" };
}

/**
 * New Contact (#3245): a name and the identities the contact is reached at,
 * each on its service. A contact may have a name and no identity.
 *
 * An identity another contact holds is refused by the server, which names that
 * contact, and nothing is created: the line under the identity says who holds
 * it, so the person removes it here or goes to that contact. No merge is
 * offered, because the new contact does not exist yet.
 *
 * The caller mounts it to open it and unmounts it to close it, so each New
 * Contact starts with an empty form.
 */
export default function NewContactDialog({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  /** The contact the server made, as `GET /v1/contacts/{id}` answers it. */
  onCreated: (contact: Contact) => void;
}) {
  const nextKey = useRef(1);
  const newRow = (): IdentityRow => ({
    key: nextKey.current++,
    service: "text_message",
    address: "",
  });
  const [name, setName] = useState("");
  const [rows, setRows] = useState<IdentityRow[]>(() => [newRow()]);
  const [rowErrors, setRowErrors] = useState<Map<number, string>>(() => new Map());
  const [formError, setFormError] = useState("");
  const nameId = useId();
  const create = useRouteMutation({ mutationFn: createContact });
  const busy = create.isPending;

  const updateRow = (key: number, change: Partial<IdentityRow>) => {
    setRows((prev) => prev.map((row) => (row.key === key ? { ...row, ...change } : row)));
    setRowErrors((prev) => {
      if (!prev.has(key)) return prev;
      const next = new Map(prev);
      next.delete(key);
      return next;
    });
  };

  const filled = rows.filter((row) => row.address.trim() !== "");
  const duplicates = new Set<number>();
  const seen = new Set<string>();
  for (const row of filled) {
    const key = identityDuplicateKey(row.service, row.address);
    if (key === null) continue;
    if (seen.has(key)) duplicates.add(row.key);
    seen.add(key);
  }
  const canSubmit = name.trim() !== "" && duplicates.size === 0 && !busy;

  const submit = () => {
    if (!canSubmit) return;
    const invalid = new Map<number, string>();
    for (const row of filled) {
      const why = identityValidationError(row.service, row.address);
      if (why) invalid.set(row.key, why);
    }
    setFormError("");
    if (invalid.size > 0) {
      setRowErrors(invalid);
      return;
    }
    setRowErrors(new Map());
    const sent = filled.map((row) => row.key);
    create.mutate(
      {
        name: name.trim(),
        identities: filled.map((row) => ({
          address: row.address.trim(),
          service: serverService(row.service),
        })),
      },
      {
        onSuccess: (contact) => onCreated(contact),
        onError: (err) => {
          const placed = placeRefusal(err, sent);
          setRowErrors(placed.rows);
          setFormError(placed.rest);
        },
      },
    );
  };

  const onEnter = (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      submit();
    }
  };

  return (
    <ModalShell
      open
      onOpenChange={(o) => {
        if (!o && !busy) onClose();
      }}
      dismissable={!busy}
      label="New Contact"
      title="New Contact"
      onClose={onClose}
      closeDisabled={busy}
      maxWidth="30rem"
    >
      <div className="mt-4 mb-4">
        <label htmlFor={nameId} className={fieldLabelClass}>
          Name
        </label>
        <input
          id={nameId}
          type="text"
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            setFormError("");
          }}
          onKeyDown={onEnter}
          disabled={busy}
          autoComplete="off"
          className={inputClass}
        />
      </div>

      <fieldset className="m-0 min-w-0 border-0 p-0">
        <legend className={fieldLabelClass}>Identities</legend>
        <ul className="m-0 flex list-none flex-col gap-2 p-0">
          {rows.map((row, index) => {
            const problem = duplicates.has(row.key) ? DUPLICATE_MESSAGE : rowErrors.get(row.key);
            const problemId = `new-contact-identity-${row.key}-problem`;
            return (
              <li key={row.key}>
                <div className="flex items-center gap-2">
                  <Select
                    selectedKey={row.service}
                    onSelectionChange={(k) => {
                      const next = parseSelectKey(k, OFFERED_SERVICES);
                      if (next) updateRow(row.key, { service: next });
                    }}
                    aria-label={`Service of identity ${index + 1}`}
                    isDisabled={busy}
                    triggerClassName={selectTriggerClass}
                    valueClassName={selectValueClass}
                    popoverClassName={Z_POPOVER_IN_MODAL}
                    className="block w-[9.5rem] shrink-0"
                  >
                    {OFFERED_SERVICE_OPTIONS.map((s) => (
                      <ListBoxItem key={s.value} id={s.value} className={selectItemClassName}>
                        {s.label}
                      </ListBoxItem>
                    ))}
                  </Select>
                  <input
                    type="text"
                    value={row.address}
                    onChange={(e) => updateRow(row.key, { address: e.target.value })}
                    onKeyDown={onEnter}
                    disabled={busy}
                    autoComplete="off"
                    spellCheck={false}
                    aria-label={`Identity ${index + 1}`}
                    placeholder={identityPlaceholder(row.service)}
                    aria-invalid={problem ? true : undefined}
                    aria-describedby={problem ? problemId : undefined}
                    className={`${inputClass} min-w-0 flex-1`}
                  />
                  <PlainButton
                    aria-label={`Remove identity ${index + 1}`}
                    title="Remove identity"
                    isDisabled={busy}
                    onPress={() => {
                      setRows((prev) => prev.filter((r) => r.key !== row.key));
                      setFormError("");
                    }}
                    className={`flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-muted hover:text-text ${focusRingClass}`}
                  >
                    <XIcon size={14} />
                  </PlainButton>
                </div>
                {problem ? (
                  <p id={problemId} className="mt-1 mb-0 text-[0.813rem] text-danger" role="alert">
                    {problem}
                  </p>
                ) : null}
              </li>
            );
          })}
        </ul>
        <Button
          size="sm"
          className="mt-2"
          isDisabled={busy}
          onPress={() => setRows((prev) => [...prev, newRow()])}
        >
          Add identity
        </Button>
      </fieldset>

      <DialogError message={formError} />

      <DialogFooter>
        <Button onPress={onClose} isDisabled={busy}>
          Cancel
        </Button>
        <Button variant="primary" onPress={submit} isDisabled={!canSubmit}>
          {busy ? "Adding…" : "Add Contact"}
        </Button>
      </DialogFooter>
    </ModalShell>
  );
}
