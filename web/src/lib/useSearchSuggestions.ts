import { useEffect, useMemo, useRef } from "react";
import { keys } from "./queryKeys";
import { useRouteQuery } from "./routeQuery";
import { type SearchField, type SearchList, useSearchFields } from "./searchFields";
import {
  forPerson,
  lastToken,
  replaceLastToken,
  suggestion as suggestionTerm,
} from "./searchQuery";
import { listContacts } from "./serverApi";
import { useDebouncedValue } from "./useDebouncedValue";

/** How long a person word's value waits after the last keystroke before contacts are asked for. */
export const CONTACT_SUGGESTION_DEBOUNCE_MS = 150;

interface ContactName {
  id: string;
  name: string;
}

/** One autocomplete entry: unique id, displayed label, text inserted into the query. */
export interface Suggestion {
  id: string;
  label: string;
  insert: string;
}

/** Words whose value is a person, so contact names are offered. */
function isPersonWord(field: SearchField | undefined): boolean {
  return field?.value_type === "person";
}

/** Autocomplete rows for a search box on one list. */
export function buildSearchSuggestions(args: {
  completingValue: boolean;
  personOp: boolean;
  lastToken: string;
  fields: SearchField[];
  contacts: ContactName[];
}): Suggestion[] {
  // A negated token keeps its minus in the text a suggestion inserts.
  const minus = args.lastToken.startsWith("-") ? "-" : "";
  const token = args.lastToken.slice(minus.length);
  const colon = token.indexOf(":");
  if (args.completingValue) {
    const word = token.slice(0, colon).toLowerCase();
    const typed = token.slice(colon + 1).toLowerCase();
    if (args.personOp) {
      return args.contacts.slice(0, 6).map((c) => ({
        id: c.id,
        label: c.name,
        // #id survives names with spaces and renames.
        insert: `${minus}${forPerson(word, c.id)} `,
      }));
    }
    const field = args.fields.find((f) => f.word === word);
    if (!field) return [];
    return field.values
      .filter((v) => v.startsWith(typed))
      .map((v) => {
        // The label is the term itself, so a value that needs quoting shows
        // the quotes the row will actually type.
        const term = `${minus}${suggestionTerm(word, v)}`;
        return { id: term, label: term, insert: `${term} ` };
      });
  }
  if (token.length === 0) return [];
  const typed = token.toLowerCase();
  return args.fields
    .filter((f) => f.word.startsWith(typed))
    .map((f) => ({ id: f.word, label: `${f.word}:`, insert: `${minus}${f.word}:` }));
}

/**
 * A typed token read as a search term: whether a value is being typed after a
 * colon, the word before it (lower-cased, without a minus), and the value
 * without its quotes.
 */
function readToken(token: string): { completingValue: boolean; word: string; valuePart: string } {
  const colon = token.indexOf(":");
  if (colon === -1) return { completingValue: false, word: "", valuePart: "" };
  return {
    completingValue: true,
    word: token.slice(0, colon).replace(/^-/, "").toLowerCase(),
    valuePart: token.slice(colon + 1).replace(/^"|"$/g, ""),
  };
}

/** Replace the token being typed with a suggestion's text, and leave the rest as typed. */
export function applySuggestionToQuery(value: string, suggestion: Suggestion): string {
  return replaceLastToken(value, suggestion.insert);
}

/**
 * Word and value autocomplete for a search box. A bare prefix completes to a
 * word the list has; a choice word offers its values; a person word fetches
 * matching contacts and inserts `word:#id`.
 */
export function useSearchSuggestions(value: string, list: SearchList | null): Suggestion[] {
  const { fields } = useSearchFields(list);

  const typed = lastToken(value);
  const typedToken = typed.text;
  const { completingValue, word } = readToken(typedToken);
  const personOp = completingValue && isPersonWord(fields.find((f) => f.word === word));
  // Which term is being typed: where it starts, and its word.
  const term = `${typed.start}:${word}`;

  // The query as it stood once typing paused, or null while no person word
  // is typed. Contacts are asked for only once the term typed then is the
  // term typed now, so a value typed for another term, a person word's
  // included, is never offered under this one.
  const settledValue = useDebouncedValue(personOp ? value : null, CONTACT_SUGGESTION_DEBOUNCE_MS);
  const settled = settledValue === null ? null : lastToken(settledValue);
  const settledToken = settled === null ? null : readToken(settled.text);
  const prefix = settledToken?.valuePart ?? "";
  const forThisTerm =
    personOp && settled !== null && `${settled.start}:${settledToken?.word}` === term;

  // The term the last answer on screen was for.
  const answeredTerm = useRef<string | null>(null);
  const { data, isPlaceholderData } = useRouteQuery(
    keys.contacts.suggest(prefix),
    (signal) => listContacts({ q: prefix, limit: 20, offset: 0 }, { signal }),
    {
      enabled: forThisTerm,
      // The last prefix's contacts stay offered while the next one is asked
      // for, so the list does not blank between keystrokes. Only within one
      // term: the next term starts from no contacts, not the last term's.
      placeholderData: (previous) => (answeredTerm.current === term ? previous : undefined),
    },
  );
  useEffect(() => {
    if (forThisTerm && data !== undefined && !isPlaceholderData) answeredTerm.current = term;
    // A term typed again at the same place after the person word went away,
    // such as after clearing the box, is a new term.
    else if (!personOp) answeredTerm.current = null;
  }, [forThisTerm, personOp, data, isPlaceholderData, term]);

  const contacts = useMemo<ContactName[]>(
    () => (forThisTerm ? (data?.items ?? []).map((c) => ({ id: String(c.id), name: c.name })) : []),
    [forThisTerm, data],
  );

  return buildSearchSuggestions({
    completingValue,
    personOp,
    lastToken: typedToken,
    fields,
    contacts,
  });
}
