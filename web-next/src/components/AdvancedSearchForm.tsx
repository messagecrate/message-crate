"use client";

import {
  composeSearchQuery,
  formFromSearchQuery,
  type AdvancedSearchForm as FormState,
  type AttachmentFilter,
  type CountFilterInput,
  type CountComparator,
  type DateFilterInput,
  type DateFilterMode,
} from "@/lib/searchQuery";
import { useState, type ReactNode } from "react";
import { ChevronDownIcon, ChevronRightIcon } from "./icons";

const inputClass =
  "w-full rounded-md border border-border bg-elevated px-2.5 py-1.5 text-[13px] text-text outline-none placeholder:text-muted focus:border-accent";
const labelClass = "w-36 shrink-0 text-[13px] text-muted";

const NO_DATES: DateFilterInput = { mode: "any", from: "", to: "" };
const SEARCH_MODE_KEY = "vault-advanced-search-mode";

function seedPersonExpanded(seed: FormState): boolean {
  return !!(
    seed.firstName ||
    seed.lastName ||
    seed.phone ||
    seed.noFirstName ||
    seed.noLastName
  );
}

function readPreferredSearchMode(): "contacts" | "messages" {
  if (typeof window === "undefined") return "contacts";
  const raw = window.localStorage.getItem(SEARCH_MODE_KEY);
  if (raw === "contacts" || raw === "messages") return raw;
  return "contacts";
}

function persistSearchMode(mode: "contacts" | "messages") {
  window.localStorage.setItem(SEARCH_MODE_KEY, mode);
}

/** Prefer the query bar's mode when set; otherwise last-picked tab (default Contacts). */
function initialSearchMode(query: string, seed: FormState): "contacts" | "messages" {
  if (/\bsearch:contacts\b/i.test(query)) return "contacts";
  if (query.trim() !== "" && seed.mode !== "contacts") return "messages";
  return readPreferredSearchMode();
}

export function AdvancedSearchForm({
  sources,
  labels,
  initialQuery,
  onSearch,
  onCancel,
}: {
  sources: string[];
  labels: string[];
  initialQuery: string;
  onSearch: (query: string) => void;
  onCancel: () => void;
}) {
  // Panel remounts each open; hydrate once from the query bar string.
  const seed = formFromSearchQuery(initialQuery);
  const [mode, setModeState] = useState<"contacts" | "messages">(() =>
    initialSearchMode(initialQuery, seed),
  );
  const setMode = (next: "contacts" | "messages") => {
    setModeState(next);
    persistSearchMode(next);
  };
  const [within, setWithin] = useState(seed.within ?? "");
  const personExpandedSeed = seedPersonExpanded(seed);
  const [handleExpanded, setHandleExpanded] = useState(personExpandedSeed);
  const [withPersonExpanded, setWithPersonExpanded] = useState(personExpandedSeed);
  const [handle, setHandle] = useState(seed.handle ?? "");
  const [firstName, setFirstName] = useState(seed.firstName ?? "");
  const [lastName, setLastName] = useState(seed.lastName ?? "");
  const [phone, setPhone] = useState(seed.phone ?? "");
  const [noFirstName, setNoFirstName] = useState(!!seed.noFirstName);
  const [noLastName, setNoLastName] = useState(!!seed.noLastName);
  const [withPerson, setWithPerson] = useState(seed.withPerson ?? "");
  const [fromPerson, setFromPerson] = useState(seed.fromPerson ?? "");
  const [toPerson, setToPerson] = useState(seed.toPerson ?? "");
  const [hasWords, setHasWords] = useState(seed.hasWords ?? "");
  const [doesntHave, setDoesntHave] = useState(seed.doesntHave ?? "");
  const [subject, setSubject] = useState(seed.subject ?? "");
  const [filename, setFilename] = useState(seed.filename ?? "");
  const [filetype, setFiletype] = useState(seed.filetype ?? "");
  const [larger, setLarger] = useState(seed.larger ?? "");
  const [smaller, setSmaller] = useState(seed.smaller ?? "");
  const [groupBy, setGroupBy] = useState<"conversation" | "none">(
    seed.groupBy ?? "conversation",
  );
  const [sort, setSort] = useState<"date-desc" | "date-asc" | "relevance">(
    seed.sort ?? "date-desc",
  );
  const [context, setContext] = useState(
    seed.context != null && seed.context > 0 ? String(seed.context) : "",
  );
  const [attachmentFilter, setAttachmentFilter] = useState<AttachmentFilter>(
    seed.attachmentFilter ?? "any",
  );
  const [date, setDate] = useState<DateFilterInput>(seed.date ?? NO_DATES);
  const [firstContact, setFirstContact] = useState<DateFilterInput>(
    seed.firstContact ?? NO_DATES,
  );
  const [lastContact, setLastContact] = useState<DateFilterInput>(
    seed.lastContact ?? NO_DATES,
  );
  const [groupCount, setGroupCount] = useState<CountFilterInput>(() => {
    const g = seed.groupCount ?? { comparator: "any" as const, value: "" };
    return g.comparator === "any" ? { comparator: "any", value: "" } : g;
  });
  const [messageCount, setMessageCount] = useState<CountFilterInput>(() => {
    const m = seed.messageCount ?? { comparator: "any" as const, value: "" };
    return m.comparator === "any" ? { comparator: "any", value: "" } : m;
  });
  const [conversationType, setConversationType] = useState<
    "any" | "group" | "one-to-one"
  >(seed.conversationType ?? "any");
  const [source, setSource] = useState(seed.source ?? "");

  const clearPersonFields = () => {
    setFirstName("");
    setLastName("");
    setPhone("");
    setNoFirstName(false);
    setNoLastName(false);
  };

  const setHandleDisclosure = (expanded: boolean) => {
    setHandleExpanded(expanded);
    if (expanded) {
      setHandle("");
    } else {
      clearPersonFields();
    }
  };

  const setWithPersonDisclosure = (expanded: boolean) => {
    setWithPersonExpanded(expanded);
    if (expanded) {
      setWithPerson("");
    } else {
      clearPersonFields();
    }
  };

  const personFields = {
    firstName: noFirstName ? undefined : firstName,
    lastName: noLastName ? undefined : lastName,
    phone,
    noFirstName: noFirstName || undefined,
    noLastName: noLastName || undefined,
  };

  const submit = () => {
    const form: FormState = {
      mode,
      within: within || undefined,
      ...(mode === "contacts"
        ? handleExpanded
          ? personFields
          : { handle }
        : withPersonExpanded
          ? personFields
          : { withPerson }),
      ...(mode !== "contacts"
        ? {
            fromPerson,
            toPerson,
            subject,
            filename,
            filetype: filetype || undefined,
            larger: larger || undefined,
            smaller: smaller || undefined,
            attachmentFilter,
            groupBy,
            sort,
            context: /^\d+$/.test(context.trim())
              ? Number.parseInt(context.trim(), 10)
              : undefined,
          }
        : {}),
      hasWords,
      doesntHave,
      date,
      firstContact,
      lastContact,
      groupCount:
        groupCount.comparator === "any"
          ? { comparator: "any", value: "" }
          : groupCount,
      messageCount:
        messageCount.comparator === "any"
          ? { comparator: "any", value: "" }
          : messageCount,
      conversationType,
      source: source || undefined,
    };
    persistSearchMode(mode);
    onSearch(composeSearchQuery(form));
  };

  return (
    <div className="border-b border-border bg-panel px-3 py-3">
      <div
        role="tablist"
        aria-label="Search type"
        className="flex border-b border-border"
      >
        {(
          [
            { id: "contacts", label: "Contacts" },
            { id: "messages", label: "Messages" },
          ] as const
        ).map((tab) => (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={mode === tab.id}
            onClick={() => setMode(tab.id)}
            className={`border-b-2 px-3 py-1.5 text-[13px] ${
              mode === tab.id
                ? "border-accent text-text"
                : "border-transparent text-muted hover:text-text"
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>
      <p className="mt-2 mb-3 text-[12px] text-muted">
        {mode === "contacts"
          ? "Find contacts by handle, or expand for first name, last name, phone, and activity."
          : "Find messages like Fastmail: from, to, with, words, attachments, and dates."}
      </p>
      <div className="space-y-2">
        {mode === "contacts" ? (
          <>
            <Field label="Within">
              <select
                className={inputClass}
                value={within}
                onChange={(e) => setWithin(e.target.value)}
              >
                <option value="">All contacts</option>
                {labels.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </select>
            </Field>
            {handleExpanded ? (
              <>
                <DisclosureRow
                  label="Handle"
                  expanded
                  onToggle={() => setHandleDisclosure(false)}
                />
                <NamePhoneFields
                  firstName={firstName}
                  lastName={lastName}
                  phone={phone}
                  noFirstName={noFirstName}
                  noLastName={noLastName}
                  onFirstNameChange={setFirstName}
                  onLastNameChange={setLastName}
                  onPhoneChange={setPhone}
                  onNoFirstNameChange={setNoFirstName}
                  onNoLastNameChange={setNoLastName}
                />
              </>
            ) : (
              <Field label="Handle">
                <div className="flex min-w-0 items-center gap-1.5">
                  <input
                    className={`${inputClass} min-w-0`}
                    value={handle}
                    onChange={(e) => setHandle(e.target.value)}
                    placeholder="Name or number"
                  />
                  <MoreButton
                    onClick={() => setHandleDisclosure(true)}
                    label="Expand handle fields"
                  />
                </div>
              </Field>
            )}
            <DateRangeField
              label="First Message"
              value={firstContact}
              onChange={setFirstContact}
            />
            <DateRangeField
              label="Last Message"
              value={lastContact}
              onChange={setLastContact}
            />
            <CountField
              label="Group message count"
              value={groupCount}
              onChange={setGroupCount}
            />
            <CountField
              label="Direct message count"
              value={messageCount}
              onChange={setMessageCount}
            />
          </>
        ) : (
          <>
            <Field label="Within">
              <select
                className={inputClass}
                value={within}
                onChange={(e) => setWithin(e.target.value)}
              >
                <option value="">All messages</option>
                {labels.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="From">
              <input
                className={inputClass}
                value={fromPerson}
                onChange={(e) => setFromPerson(e.target.value)}
                placeholder="me or name/number"
              />
            </Field>
            <Field label="To">
              <input
                className={inputClass}
                value={toPerson}
                onChange={(e) => setToPerson(e.target.value)}
                placeholder="me or name/number"
              />
            </Field>
            {withPersonExpanded ? (
              <>
                <DisclosureRow
                  label="With person"
                  expanded
                  onToggle={() => setWithPersonDisclosure(false)}
                />
                <NamePhoneFields
                  firstName={firstName}
                  lastName={lastName}
                  phone={phone}
                  noFirstName={noFirstName}
                  noLastName={noLastName}
                  onFirstNameChange={setFirstName}
                  onLastNameChange={setLastName}
                  onPhoneChange={setPhone}
                  onNoFirstNameChange={setNoFirstName}
                  onNoLastNameChange={setNoLastName}
                />
              </>
            ) : (
              <Field label="With person">
                <div className="flex min-w-0 items-center gap-1.5">
                  <input
                    className={`${inputClass} min-w-0`}
                    value={withPerson}
                    onChange={(e) => setWithPerson(e.target.value)}
                    placeholder="Name or number"
                  />
                  <MoreButton
                    onClick={() => setWithPersonDisclosure(true)}
                    label="Expand with person fields"
                  />
                </div>
              </Field>
            )}
            <Field label="Has the words">
              <input
                className={inputClass}
                value={hasWords}
                onChange={(e) => setHasWords(e.target.value)}
              />
            </Field>
            <Field label="Doesn't have">
              <input
                className={inputClass}
                value={doesntHave}
                onChange={(e) => setDoesntHave(e.target.value)}
              />
            </Field>
            <Field label="Subject">
              <input
                className={inputClass}
                value={subject}
                onChange={(e) => setSubject(e.target.value)}
                placeholder="Optional"
              />
            </Field>
            <DateRangeField label="Date" value={date} onChange={setDate} />
            <Field label="Message type">
              <select
                className={inputClass}
                value={conversationType}
                onChange={(e) =>
                  setConversationType(
                    e.target.value as "any" | "group" | "one-to-one",
                  )
                }
              >
                <option value="any">All conversations</option>
                <option value="one-to-one">Direct only</option>
                <option value="group">Group only</option>
              </select>
            </Field>
            <Field label="Source">
              <select
                className={inputClass}
                value={source}
                onChange={(e) => setSource(e.target.value)}
              >
                <option value="">Any source</option>
                {sources.map((s) => (
                  <option key={s} value={s}>
                    {s}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="Attachment">
              <select
                className={inputClass}
                value={attachmentFilter}
                onChange={(e) =>
                  setAttachmentFilter(e.target.value as AttachmentFilter)
                }
              >
                <option value="any">Any</option>
                <option value="yes">Has attachment</option>
                <option value="no">No attachment</option>
              </select>
            </Field>
            {attachmentFilter === "yes" ? (
              <>
                <Field label="File type">
                  <select
                    className={inputClass}
                    value={filetype}
                    onChange={(e) => setFiletype(e.target.value)}
                  >
                    <option value="">Any type</option>
                    <option value="image">Image</option>
                    <option value="video">Video</option>
                    <option value="audio">Audio</option>
                    <option value="document">Document</option>
                    <option value="contact">Contact</option>
                    <option value="other">Other</option>
                  </select>
                </Field>
                <Field label="Filename">
                  <input
                    className={inputClass}
                    value={filename}
                    onChange={(e) => setFilename(e.target.value)}
                    placeholder="Contains…"
                  />
                </Field>
                <Field label="Larger than">
                  <input
                    className={inputClass}
                    value={larger}
                    onChange={(e) => setLarger(e.target.value)}
                    placeholder="e.g. 1M"
                  />
                </Field>
                <Field label="Smaller than">
                  <input
                    className={inputClass}
                    value={smaller}
                    onChange={(e) => setSmaller(e.target.value)}
                    placeholder="e.g. 500k"
                  />
                </Field>
              </>
            ) : null}
            <Field label="Results">
              <select
                className={inputClass}
                value={groupBy}
                onChange={(e) =>
                  setGroupBy(e.target.value as "conversation" | "none")
                }
              >
                <option value="conversation">One row per conversation</option>
                <option value="none">One row per message</option>
              </select>
            </Field>
            <Field label="Sort">
              <select
                className={inputClass}
                value={sort}
                onChange={(e) =>
                  setSort(
                    e.target.value as "date-desc" | "date-asc" | "relevance",
                  )
                }
              >
                <option value="date-desc">Newest first</option>
                <option value="date-asc">Oldest first</option>
                <option value="relevance">Best match</option>
              </select>
            </Field>
            <Field label="Context">
              <input
                className={inputClass}
                value={context}
                onChange={(e) => setContext(e.target.value)}
                placeholder="Surrounding messages (e.g. 2)"
                inputMode="numeric"
              />
            </Field>
          </>
        )}
      </div>
      <div className="mt-3 flex items-center justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
          className="rounded-md border border-border bg-elevated px-3 py-1.5 text-[13px] text-text hover:bg-hover"
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={submit}
          className="rounded-md bg-accent px-3 py-1.5 text-[13px] font-medium text-sent-text hover:opacity-90"
        >
          Search
        </button>
      </div>
    </div>
  );
}

function MoreButton({
  onClick,
  label,
}: {
  onClick: () => void;
  label: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="inline-flex shrink-0 items-center gap-0.5 rounded-md px-1.5 py-1.5 text-[12px] text-muted hover:bg-hover hover:text-text"
      aria-expanded={false}
      aria-label={label}
    >
      <ChevronRightIcon className="size-3" />
      More
    </button>
  );
}

function DisclosureRow({
  label,
  expanded,
  onToggle,
}: {
  label: string;
  expanded: boolean;
  onToggle: () => void;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className={labelClass}>{label}</span>
      <button
        type="button"
        onClick={onToggle}
        className="inline-flex items-center gap-1 rounded-md px-1.5 py-1 text-[12px] text-muted hover:bg-hover hover:text-text"
        aria-expanded={expanded}
        aria-label={`Collapse ${label.toLowerCase()} fields`}
      >
        <ChevronDownIcon className="size-3" />
        Less
      </button>
    </div>
  );
}

function NamePhoneFields({
  firstName,
  lastName,
  phone,
  noFirstName,
  noLastName,
  onFirstNameChange,
  onLastNameChange,
  onPhoneChange,
  onNoFirstNameChange,
  onNoLastNameChange,
}: {
  firstName: string;
  lastName: string;
  phone: string;
  noFirstName: boolean;
  noLastName: boolean;
  onFirstNameChange: (value: string) => void;
  onLastNameChange: (value: string) => void;
  onPhoneChange: (value: string) => void;
  onNoFirstNameChange: (value: boolean) => void;
  onNoLastNameChange: (value: boolean) => void;
}) {
  return (
    <>
      <Field label="First name">
        <div className="flex min-w-0 items-center gap-2">
          <input
            className={`${inputClass} min-w-0 disabled:opacity-40`}
            value={noFirstName ? "" : firstName}
            disabled={noFirstName}
            onChange={(e) => onFirstNameChange(e.target.value)}
            placeholder="Contains…"
          />
          <label className="flex shrink-0 items-center gap-1.5 text-[12px] text-text">
            <input
              type="checkbox"
              checked={noFirstName}
              onChange={(e) => {
                const next = e.target.checked;
                onNoFirstNameChange(next);
                if (next) onFirstNameChange("");
              }}
              className="size-3.5 accent-accent"
            />
            No first name
          </label>
        </div>
      </Field>
      <Field label="Last name">
        <div className="flex min-w-0 items-center gap-2">
          <input
            className={`${inputClass} min-w-0 disabled:opacity-40`}
            value={noLastName ? "" : lastName}
            disabled={noLastName}
            onChange={(e) => onLastNameChange(e.target.value)}
            placeholder="Contains…"
          />
          <label className="flex shrink-0 items-center gap-1.5 text-[12px] text-text">
            <input
              type="checkbox"
              checked={noLastName}
              onChange={(e) => {
                const next = e.target.checked;
                onNoLastNameChange(next);
                if (next) onLastNameChange("");
              }}
              className="size-3.5 accent-accent"
            />
            No last name
          </label>
        </div>
      </Field>
      <Field label="Phone">
        <input
          className={inputClass}
          value={phone}
          onChange={(e) => onPhoneChange(e.target.value)}
          placeholder="Number or email"
        />
      </Field>
    </>
  );
}

function CountField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: CountFilterInput;
  onChange: (next: CountFilterInput) => void;
}) {
  return (
    <Field label={label}>
      <div className="grid min-w-0 grid-cols-[7rem_minmax(0,1fr)] gap-1.5">
        <select
          className={`${inputClass} min-w-0`}
          value={value.comparator}
          aria-label={`${label} comparison`}
          onChange={(e) => {
            const comparator = e.target.value as CountComparator | "any";
            onChange({
              comparator,
              // Clear the number when comparison is unused.
              value: comparator === "any" ? "" : (value.value ?? ""),
            });
          }}
        >
          <option value="any">Any</option>
          <option value="=">Equal to</option>
          <option value=">">More than</option>
          <option value="<">Less than</option>
        </select>
        <input
          type="number"
          min="0"
          step="1"
          className={`${inputClass} min-w-0 disabled:opacity-40`}
          value={value.comparator === "any" ? "" : (value.value ?? "")}
          disabled={value.comparator === "any"}
          aria-label={`${label} value`}
          onChange={(e) => onChange({ ...value, value: e.target.value })}
        />
      </div>
    </Field>
  );
}

/** Mode picker plus the one or two date inputs that mode needs. */
function DateRangeField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: DateFilterInput;
  onChange: (next: DateFilterInput) => void;
}) {
  const showFrom = value.mode === "on-or-after" || value.mode === "between";
  const showTo = value.mode === "before" || value.mode === "between";

  return (
    <div className="flex items-start gap-2">
      <span className={`${labelClass} pt-1.5`}>{label}</span>
      <div className="min-w-0 flex-1 space-y-1.5">
        <select
          className={inputClass}
          value={value.mode}
          aria-label={`${label} comparison`}
          onChange={(e) =>
            onChange({ ...value, mode: e.target.value as DateFilterMode })
          }
        >
          <option value="any">Any time</option>
          <option value="on-or-after">On or after</option>
          <option value="before">Before</option>
          <option value="between">Between</option>
        </select>
        {showFrom || showTo ? (
          <div className="flex items-center gap-1.5">
            {showFrom ? (
              <input
                type="date"
                className={inputClass}
                aria-label={
                  value.mode === "between" ? `${label} start` : `${label} date`
                }
                value={value.from ?? ""}
                onChange={(e) => onChange({ ...value, from: e.target.value })}
              />
            ) : null}
            {value.mode === "between" ? (
              <span className="shrink-0 text-[12px] text-muted">to</span>
            ) : null}
            {showTo ? (
              <input
                type="date"
                className={inputClass}
                aria-label={
                  value.mode === "between" ? `${label} end` : `${label} date`
                }
                value={value.to ?? ""}
                onChange={(e) => onChange({ ...value, to: e.target.value })}
              />
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className={labelClass}>{label}</span>
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}
