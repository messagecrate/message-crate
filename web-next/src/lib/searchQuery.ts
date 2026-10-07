/**
 * Vault search query parser / composer.
 *
 * Behavior reference for the shared grammar (Rust must match):
 * `tests/fixtures/search/parse-cases.json`. After intentional grammar changes, run
 * `node scripts/deprecated/regen-search-goldens.mjs` and fix Rust until both
 * golden suites pass.
 *
 * Supported operators:
 *   search:contacts  handle:  first:  last:  phone:  is:nofirst  is:nolast
 *   is:nameless (legacy → both nofirst and nolast)
 *   first:/last:/phone:/nofirst/nolast also scope Messages “with person”
 *   within:  last-contact:  first-contact:  group-count:  message-count:
 *   from:  to:  with:  subject:  text:  has:attachment|noattachment
 *   filename:  filetype:  larger:  smaller:  in:  after:  before:  source:
 *   is:group  is:direct
 *   group:none|conversation  context:N  sort:date-asc|date|relevance
 *   "quoted phrases"  -term  NOT term
 *   OR / AND / (grouping) / prefix*  among free-text (FTS MATCH only)
 *
 * `from:me` = sent by you (`is_from_me`). Other `from:` = sender match.
 * `to:` = addressed to (sent by me to them, or `to:me` = received).
 * `with:` = conversation involves this person (any participant).
 * `in:` = restrict to a conversation (title / handle); `in:trash` ignored.
 * `within:` / `label:` = contacts on one label.
 *
 * `after:` / `before:` accept YYYY, YYYY-MM-DD, or relative `7d` / `1w` / `1m` / `1y`.
 *
 * `first-contact:` / `last-contact:` bound a contact's overall first / last
 * message date (`>=D`, `<D`, `D1..D2`, bare `D` = before).
 */

/** Inclusive lower bound / upper bound, both `YYYY-MM-DD`. */
export type DateBounds = {
  from: string | null;
  to: string | null;
};

/** Free-text boolean expression compiled to FTS5 MATCH. */
export type FtsNode =
  | { type: "term"; value: string; prefix?: boolean }
  | { type: "phrase"; value: string }
  | { type: "and"; children: FtsNode[] }
  | { type: "or"; children: FtsNode[] }
  | { type: "not"; child: FtsNode };

export type ParsedSearchQuery = {
  /** Queries without an explicit search operator retain legacy message mode. */
  mode: "messages" | "contacts";
  /** Free-text terms (leaf nodes; for highlights / simple AND). */
  terms: string[];
  /** Phrases that appear in the free-text expression. */
  phrases: string[];
  /** Terms/phrases excluded via -term or NOT. */
  exclude: string[];
  /** Boolean free-text expression for FTS5 MATCH (OR / AND / NOT / groups / prefix). */
  ftsAst: FtsNode | null;
  /** Sender filter; use `me` for messages you sent. */
  from: string | null;
  /** Addressed-to filter; use `me` for received messages. */
  to: string | null;
  /** Conversation involves this person (sender or recipient). */
  with: string | null;
  subject: string | null;
  /** Explicit body-only FTS terms. */
  text: string | null;
  /** null = any, true = has attachment, false = no attachment. */
  hasAttachment: boolean | null;
  filename: string | null;
  /** image | video | audio | document | contact | other | pdf (→ document). */
  filetype: string | null;
  /** Minimum attachment size in bytes (`larger:`). */
  largerBytes: number | null;
  /** Maximum attachment size in bytes (`smaller:`). */
  smallerBytes: number | null;
  /** Restrict to a conversation by title or handle (`in:`). */
  inConversation: string | null;
  after: string | null;
  before: string | null;
  source: string | null;
  conversationType: "group" | "individual" | null;
  /** Conversation-grouped (default) or one row per matching message. */
  groupBy: "conversation" | "none";
  /** Surrounding messages to include when showing/opening a hit. */
  context: number;
  /** Result ordering. */
  sort: "date-desc" | "date-asc" | "relevance";
  /** Label whose contacts to search. */
  within: string | null;
  /** Contact handle (name or phone number). Legacy combined filter. */
  handle: string | null;
  /** Substring match on contact first name. */
  firstName: string | null;
  /** Substring match on contact last name. */
  lastName: string | null;
  /** Substring match on phone / email handles only. */
  phone: string | null;
  /** Contacts with empty first name. */
  noFirstName: boolean;
  /** Contacts with empty last name. */
  noLastName: boolean;
  /** Bounds on each contact's overall last message date. */
  lastContact: DateBounds;
  /** Bounds on each contact's overall first message date. */
  firstContact: DateBounds;
  groupCount: CountComparison | null;
  messageCount: CountComparison | null;
  /** @deprecated Retained for untouched legacy consumers; always false. */
  showContact: boolean;
};

/** Which bound(s) a date field contributes. */
export type DateFilterMode = "any" | "on-or-after" | "before" | "between";

export type DateFilterInput = {
  mode: DateFilterMode;
  from?: string;
  to?: string;
};

export type CountComparator = "=" | ">" | ">=" | "<" | "<=";

export type CountComparison = {
  comparator: CountComparator;
  value: number;
};

export type CountFilterInput = {
  comparator: CountComparator | "any";
  value?: string;
};

export type AttachmentFilter = "any" | "yes" | "no";

export type AdvancedSearchForm = {
  mode?: "messages" | "contacts";
  /** Label to search within; empty means all contacts. */
  within?: string;
  /** Name or number of a conversation participant (`with:`). */
  withPerson?: string;
  /** Sender (`from:`); use `me` for messages you sent. */
  fromPerson?: string;
  /** Addressed-to (`to:`); use `me` for received. */
  toPerson?: string;
  /** Contact handle (name or phone number). Legacy combined filter. */
  handle?: string;
  firstName?: string;
  lastName?: string;
  phone?: string;
  /** Contacts search: only contacts with empty first name. */
  noFirstName?: boolean;
  /** Contacts search: only contacts with empty last name. */
  noLastName?: boolean;
  hasWords?: string;
  doesntHave?: string;
  subject?: string;
  filename?: string;
  filetype?: string;
  /** Human size for `larger:` (e.g. `1M`, `500k`). */
  larger?: string;
  /** Human size for `smaller:`. */
  smaller?: string;
  /** Conversation title / handle (`in:`). */
  inConversation?: string;
  /** `group:none` when "messages". */
  groupBy?: "conversation" | "none";
  /** Surrounding messages (`context:N`). */
  context?: number;
  /** Result sort (`sort:`). */
  sort?: "date-desc" | "date-asc" | "relevance";
  /** Message timestamp bounds. */
  date?: DateFilterInput;
  /** Contact's first message date bounds. */
  firstContact?: DateFilterInput;
  /** Contact's last message date bounds. */
  lastContact?: DateFilterInput;
  groupCount?: CountFilterInput;
  messageCount?: CountFilterInput;
  conversationType?: "any" | "group" | "individual";
  source?: string;
  /** @deprecated Prefer attachmentFilter; true maps to "yes". */
  hasAttachment?: boolean;
  attachmentFilter?: AttachmentFilter;
};

const NO_BOUNDS: DateBounds = { from: null, to: null };

const EMPTY: ParsedSearchQuery = {
  mode: "messages",
  terms: [],
  phrases: [],
  exclude: [],
  ftsAst: null,
  from: null,
  to: null,
  with: null,
  subject: null,
  text: null,
  hasAttachment: null,
  filename: null,
  filetype: null,
  largerBytes: null,
  smallerBytes: null,
  inConversation: null,
  after: null,
  before: null,
  source: null,
  conversationType: null,
  groupBy: "conversation",
  context: 0,
  sort: "date-desc",
  within: null,
  handle: null,
  firstName: null,
  lastName: null,
  phone: null,
  noFirstName: false,
  noLastName: false,
  lastContact: NO_BOUNDS,
  firstContact: NO_BOUNDS,
  groupCount: null,
  messageCount: null,
  showContact: false,
};

const OPERATOR_RE =
  /^(search|with|from|to|subject|text|has|after|before|source|is|within|label|in|show|handle|filename|filetype|larger|smaller|group-count|message-count|group|context|sort|last-contact|first-contact|first|last|phone):(.*)$/i;

/** Parse `500k` / `1.5M` / `2G` / plain bytes into an integer byte count. */
export function parseSizeBytes(raw: string): number | null {
  const t = raw.trim().toLowerCase().replace(/,/g, "");
  if (!t) return null;
  const m = t.match(/^(\d+(?:\.\d+)?)\s*([kmgt]?b?)?$/i);
  if (!m) return null;
  const n = Number.parseFloat(m[1]!);
  if (!Number.isFinite(n) || n < 0) return null;
  const unit = (m[2] || "").replace(/b$/i, "").toLowerCase();
  const mult =
    unit === "k" ? 1024
    : unit === "m" ? 1024 ** 2
    : unit === "g" ? 1024 ** 3
    : unit === "t" ? 1024 ** 4
    : 1;
  return Math.round(n * mult);
}

/** Format bytes for query composition (`larger:` / `smaller:`). */
export function formatSizeBytes(bytes: number): string {
  if (bytes >= 1024 ** 3 && bytes % 1024 ** 3 === 0) return `${bytes / 1024 ** 3}G`;
  if (bytes >= 1024 ** 2 && bytes % 1024 ** 2 === 0) return `${bytes / 1024 ** 2}M`;
  if (bytes >= 1024 && bytes % 1024 === 0) return `${bytes / 1024}K`;
  return String(bytes);
}

function readQuoted(s: string, start: number): { value: string; next: number } {
  let i = start;
  let phrase = "";
  while (i < s.length && s[i] !== '"') {
    phrase += s[i];
    i += 1;
  }
  if (i < s.length && s[i] === '"') i += 1;
  return { value: phrase, next: i };
}

function tokenize(input: string): string[] {
  const tokens: string[] = [];
  let i = 0;
  const s = input.trim();
  while (i < s.length) {
    while (i < s.length && /\s/.test(s[i]!)) i += 1;
    if (i >= s.length) break;

    // Parentheses are their own tokens (FTS grouping).
    if (s[i] === "(" || s[i] === ")") {
      tokens.push(s[i]!);
      i += 1;
      continue;
    }

    // Negated quoted phrase: -"bad word"
    if (s[i] === "-" && s[i + 1] === '"') {
      const { value, next } = readQuoted(s, i + 2);
      tokens.push(`-"${value}"`);
      i = next;
      continue;
    }

    // Quoted phrase: "exact phrase"
    if (s[i] === '"') {
      const { value, next } = readQuoted(s, i + 1);
      tokens.push(`"${value}"`);
      i = next;
      continue;
    }

    let tok = "";
    while (i < s.length && !/\s/.test(s[i]!) && s[i] !== "(" && s[i] !== ")") {
      // Operator with quoted value: from:"Ann Lee"
      if (s[i] === ":" && s[i + 1] === '"') {
        tok += ':"';
        const { value, next } = readQuoted(s, i + 2);
        tok += `${value}"`;
        i = next;
        break;
      }
      tok += s[i];
      i += 1;
    }
    if (tok) tokens.push(tok);
  }
  return tokens;
}

type FtsLex =
  | { kind: "term"; value: string; prefix: boolean }
  | { kind: "phrase"; value: string }
  | { kind: "or" }
  | { kind: "and" }
  | { kind: "not" }
  | { kind: "lparen" }
  | { kind: "rparen" };

/** Convert one non-operator token into FTS lexeme(s); leading `-` becomes NOT. */
function appendFtsLexemes(token: string, out: FtsLex[]): void {
  if (token === "(") {
    out.push({ kind: "lparen" });
    return;
  }
  if (token === ")") {
    out.push({ kind: "rparen" });
    return;
  }
  const upper = token.toUpperCase();
  if (upper === "OR") {
    out.push({ kind: "or" });
    return;
  }
  if (upper === "AND") {
    out.push({ kind: "and" });
    return;
  }
  if (upper === "NOT") {
    out.push({ kind: "not" });
    return;
  }

  let raw = token;
  if (raw.startsWith("-") && raw.length > 1) {
    out.push({ kind: "not" });
    raw = raw.slice(1);
  }

  if (raw.startsWith('"') && raw.endsWith('"') && raw.length >= 2) {
    const phrase = raw.slice(1, -1).trim();
    if (phrase) out.push({ kind: "phrase", value: phrase });
    return;
  }

  let prefix = false;
  let value = raw;
  if (value.endsWith("*") && value.length > 1) {
    prefix = true;
    value = value.slice(0, -1);
  }
  if (value) out.push({ kind: "term", value, prefix });
}

/** Parse free-text lexemes into an AST. AND binds tighter than OR; NOT is unary. */
function parseFtsLexemes(lexemes: FtsLex[]): FtsNode | null {
  let i = 0;

  function peek(): FtsLex | undefined {
    return lexemes[i];
  }
  function consume(): FtsLex | undefined {
    return lexemes[i++];
  }

  function parsePrimary(): FtsNode | null {
    const tok = peek();
    if (!tok) return null;
    if (tok.kind === "lparen") {
      consume();
      const inner = parseOr();
      if (peek()?.kind === "rparen") consume();
      return inner;
    }
    if (tok.kind === "term") {
      consume();
      return { type: "term", value: tok.value, prefix: tok.prefix || undefined };
    }
    if (tok.kind === "phrase") {
      consume();
      return { type: "phrase", value: tok.value };
    }
    return null;
  }

  function parseUnary(): FtsNode | null {
    const tok = peek();
    if (tok?.kind === "not") {
      consume();
      const child = parseUnary();
      return child ? { type: "not", child } : null;
    }
    return parsePrimary();
  }

  function parseAnd(): FtsNode | null {
    const nodes: FtsNode[] = [];
    const first = parseUnary();
    if (!first) return null;
    nodes.push(first);
    while (true) {
      const tok = peek();
      if (!tok || tok.kind === "or" || tok.kind === "rparen") break;
      if (tok.kind === "and") {
        consume();
        const next = parseUnary();
        if (!next) break;
        nodes.push(next);
        continue;
      }
      // Implicit AND before another primary / NOT / (
      if (
        tok.kind === "not" ||
        tok.kind === "lparen" ||
        tok.kind === "term" ||
        tok.kind === "phrase"
      ) {
        const next = parseUnary();
        if (!next) break;
        nodes.push(next);
        continue;
      }
      break;
    }
    if (nodes.length === 1) return nodes[0]!;
    return { type: "and", children: nodes };
  }

  function parseOr(): FtsNode | null {
    const nodes: FtsNode[] = [];
    const first = parseAnd();
    if (!first) return null;
    nodes.push(first);
    while (peek()?.kind === "or") {
      consume();
      const next = parseAnd();
      if (!next) break;
      nodes.push(next);
    }
    if (nodes.length === 1) return nodes[0]!;
    return { type: "or", children: nodes };
  }

  const ast = parseOr();
  return ast;
}

function flattenFtsLeaves(
  node: FtsNode | null,
  into: { terms: string[]; phrases: string[]; exclude: string[] },
  negated = false,
): void {
  if (!node) return;
  switch (node.type) {
    case "term":
      if (negated) into.exclude.push(node.value);
      else into.terms.push(node.value);
      break;
    case "phrase":
      if (negated) into.exclude.push(node.value);
      else into.phrases.push(node.value);
      break;
    case "not":
      flattenFtsLeaves(node.child, into, !negated);
      break;
    case "and":
    case "or":
      for (const child of node.children) flattenFtsLeaves(child, into, negated);
      break;
  }
}

function compileFtsNode(node: FtsNode): string | null {
  switch (node.type) {
    case "term": {
      const safe = node.value.replace(/"/g, "").trim();
      if (!safe) return null;
      if (node.prefix) {
        // FTS5 prefix queries must be unquoted bare tokens.
        const bare = safe.replace(/\s+/g, "");
        return bare ? `${bare}*` : null;
      }
      return `"${safe}"`;
    }
    case "phrase": {
      const safe = node.value.replace(/"/g, "").trim();
      return safe ? `"${safe}"` : null;
    }
    case "not": {
      const inner = compileFtsNode(node.child);
      return inner ? `NOT ${inner}` : null;
    }
    case "and": {
      const parts = node.children
        .map(compileFtsNode)
        .filter((p): p is string => !!p);
      if (parts.length === 0) return null;
      if (parts.length === 1) return parts[0]!;
      return parts
        .map((p) => {
          if (p.startsWith("(") && p.endsWith(")")) return p;
          return p.includes(" OR ") ? `(${p})` : p;
        })
        .join(" AND ");
    }
    case "or": {
      const parts = node.children
        .map(compileFtsNode)
        .filter((p): p is string => !!p);
      if (parts.length === 0) return null;
      if (parts.length === 1) return parts[0]!;
      return `(${parts
        .map((p) => {
          if (p.startsWith("(") && p.endsWith(")")) return p;
          return p.includes(" AND ") ? `(${p})` : p;
        })
        .join(" OR ")})`;
    }
  }
}

/** Serialize an FTS AST back into query-bar free text (for the advanced form). */
export function serializeFtsAst(node: FtsNode): string {
  switch (node.type) {
    case "term":
      return node.prefix ? `${node.value}*` : node.value;
    case "phrase":
      return quoteToken(node.value);
    case "not": {
      const inner = serializeFtsAst(node.child);
      if (node.child.type === "term" || node.child.type === "phrase") {
        return `-${inner}`;
      }
      return `NOT (${inner})`;
    }
    case "and":
      return node.children.map(serializeFtsAst).join(" ");
    case "or":
      return node.children
        .map((child) => {
          const s = serializeFtsAst(child);
          return child.type === "and" ? `(${s})` : s;
        })
        .join(" OR ");
  }
}

/** Absolute YYYY / YYYY-MM-DD, or relative Nd/Nw/Nm/Ny → local calendar YYYY-MM-DD. */
function normalizeDate(raw: string): string | null {
  const t = raw.trim();
  if (!t) return null;
  const rel = t.match(/^(\d+)([dwmy])$/i);
  if (rel) {
    const n = Number.parseInt(rel[1]!, 10);
    if (!Number.isFinite(n) || n < 0) return null;
    const unit = rel[2]!.toLowerCase();
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    if (unit === "d") d.setDate(d.getDate() - n);
    else if (unit === "w") d.setDate(d.getDate() - n * 7);
    else if (unit === "m") d.setMonth(d.getMonth() - n);
    else d.setFullYear(d.getFullYear() - n);
    const yyyy = d.getFullYear();
    const mm = String(d.getMonth() + 1).padStart(2, "0");
    const dd = String(d.getDate()).padStart(2, "0");
    return `${yyyy}-${mm}-${dd}`;
  }
  if (/^\d{4}-\d{2}-\d{2}$/.test(t)) return t;
  if (/^\d{4}$/.test(t)) return `${t}-01-01`;
  return t || null;
}

/** Normalize filetype aliases (e.g. pdf → document). */
export function normalizeFiletype(raw: string): string | null {
  const v = raw.trim().toLowerCase();
  if (!v) return null;
  if (v === "pdf") return "document";
  if (
    v === "image" ||
    v === "video" ||
    v === "audio" ||
    v === "document" ||
    v === "contact" ||
    v === "other"
  ) {
    return v;
  }
  return v;
}

/** Read `>=D`, `<D`, `D1..D2`, or a bare `D` (which means “before D”). */
function parseDateBounds(raw: string): DateBounds {
  const t = raw.trim();
  if (!t) return NO_BOUNDS;

  const range = t.match(/^(.+?)\.\.(.+)$/);
  if (range) {
    return { from: normalizeDate(range[1]!), to: normalizeDate(range[2]!) };
  }
  if (t.startsWith(">=")) return { from: normalizeDate(t.slice(2)), to: null };
  if (t.startsWith(">")) return { from: normalizeDate(t.slice(1)), to: null };
  if (t.startsWith("<=")) return { from: null, to: normalizeDate(t.slice(2)) };
  if (t.startsWith("<")) return { from: null, to: normalizeDate(t.slice(1)) };
  return { from: null, to: normalizeDate(t) };
}

function parseCountComparison(raw: string): CountComparison | null {
  const match = raw.trim().match(/^(>=|<=|>|<|=)(\d+)$/);
  if (!match) return null;
  return {
    comparator: match[1] as CountComparator,
    value: Number.parseInt(match[2]!, 10),
  };
}

export function hasDateBounds(bounds: DateBounds): boolean {
  return !!bounds.from || !!bounds.to;
}

export function parseSearchQuery(input: string): ParsedSearchQuery {
  const out: ParsedSearchQuery = {
    ...EMPTY,
    terms: [],
    phrases: [],
    exclude: [],
    ftsAst: null,
    lastContact: { ...NO_BOUNDS },
    firstContact: { ...NO_BOUNDS },
  };
  if (!input.trim()) return out;

  const ftsLexemes: FtsLex[] = [];

  for (const raw of tokenize(input)) {
    // Operators bind as global AND filters; free text builds an FTS AST.
    const m = raw.match(OPERATOR_RE);
    if (m) {
      const op = m[1]!.toLowerCase();
      const value = m[2]!.trim().replace(/^"|"$/g, "");
      if (!value && op !== "has") continue;
      switch (op) {
        case "search": {
          const mode = value.toLowerCase();
          if (mode === "contacts" || mode === "messages") out.mode = mode;
          break;
        }
        case "from":
          out.from = value;
          break;
        case "to":
          out.to = value;
          break;
        case "with":
          out.with = value;
          break;
        case "subject":
          out.subject = value;
          break;
        case "text":
          out.text = value;
          break;
        case "has": {
          const v = value.toLowerCase();
          if (v === "attachment" || v === "att") out.hasAttachment = true;
          else if (v === "noattachment" || v === "noatt") {
            out.hasAttachment = false;
          }
          break;
        }
        case "after":
          out.after = normalizeDate(value);
          break;
        case "before":
          out.before = normalizeDate(value);
          break;
        case "source":
          out.source = value;
          break;
        case "is": {
          const v = value.toLowerCase();
          if (v === "group") out.conversationType = "group";
          else if (v === "direct" || v === "individual" || v === "1-1") {
            out.conversationType = "individual";
          } else if (v === "nofirst") {
            out.noFirstName = true;
          } else if (v === "nolast") {
            out.noLastName = true;
          } else if (v === "nameless") {
            // Legacy: both empty.
            out.noFirstName = true;
            out.noLastName = true;
          }
          break;
        }
        case "within":
        case "label":
          out.within = value;
          break;
        case "handle":
          out.handle = value;
          break;
        case "first":
          out.firstName = value;
          break;
        case "last":
          out.lastName = value;
          break;
        case "phone":
          out.phone = value;
          break;
        case "filename":
          out.filename = value;
          break;
        case "filetype":
          out.filetype = normalizeFiletype(value);
          break;
        case "larger":
          out.largerBytes = parseSizeBytes(value);
          break;
        case "smaller":
          out.smallerBytes = parseSizeBytes(value);
          break;
        case "group": {
          const v = value.toLowerCase();
          if (v === "none" || v === "messages" || v === "message") {
            out.groupBy = "none";
          } else if (v === "conversation" || v === "conversations") {
            out.groupBy = "conversation";
          }
          break;
        }
        case "context": {
          const n = Number.parseInt(value, 10);
          if (Number.isFinite(n) && n >= 0) {
            out.context = Math.min(n, 20);
          }
          break;
        }
        case "sort": {
          const v = value.toLowerCase();
          if (v === "date-asc" || v === "oldest" || v === "asc") {
            out.sort = "date-asc";
          } else if (
            v === "relevance" ||
            v === "rank" ||
            v === "best"
          ) {
            out.sort = "relevance";
          } else if (
            v === "date" ||
            v === "date-desc" ||
            v === "newest" ||
            v === "desc"
          ) {
            out.sort = "date-desc";
          }
          break;
        }
        case "in":
          // Legacy in:trash — trash is always excluded now.
          if (value.toLowerCase() !== "trash") out.inConversation = value;
          break;
        case "show":
          // Retired legacy presentation operator.
          break;
        case "last-contact":
          out.lastContact = parseDateBounds(value);
          break;
        case "first-contact":
          out.firstContact = parseDateBounds(value);
          break;
        case "group-count":
          out.groupCount = parseCountComparison(value);
          break;
        case "message-count":
          out.messageCount = parseCountComparison(value);
          break;
        default:
          break;
      }
      continue;
    }

    appendFtsLexemes(raw, ftsLexemes);
  }

  out.ftsAst = parseFtsLexemes(ftsLexemes);
  flattenFtsLeaves(out.ftsAst, out);
  return out;
}

/** Serialize a date field back to its operator value (`>=D`, `<D`, `D1..D2`). */
function composeDateBounds(input: DateFilterInput | undefined): string | null {
  if (!input || input.mode === "any") return null;
  const from = input.from?.trim() || "";
  const to = input.to?.trim() || "";
  if (input.mode === "on-or-after") return from ? `>=${from}` : null;
  if (input.mode === "before") return to ? `<${to}` : null;
  if (from && to) return `${from}..${to}`;
  // Partly filled "between" still narrows on the side that has a date.
  if (from) return `>=${from}`;
  if (to) return `<${to}`;
  return null;
}

function quoteToken(value: string): string {
  return /\s/.test(value) ? `"${value.replace(/"/g, "")}"` : value;
}

function composeCountComparison(input: CountFilterInput | undefined): string | null {
  if (!input || input.comparator === "any") return null;
  const value = input.value?.trim() ?? "";
  if (!/^\d+$/.test(value)) return null;
  return `${input.comparator}${value}`;
}

/** Inverse of composeDateBounds / after+before pairing. */
function dateFilterFromBounds(from: string | null, to: string | null): DateFilterInput {
  if (from && to) return { mode: "between", from, to };
  if (from) return { mode: "on-or-after", from, to: "" };
  if (to) return { mode: "before", from: "", to };
  return { mode: "any", from: "", to: "" };
}

/**
 * Reverse-parse a vault search string into advanced-form fields so reopening
 * the dropdown can restore fields from the query bar.
 */
export function formFromSearchQuery(query: string): AdvancedSearchForm {
  const parsed = parseSearchQuery(query);
  // Prefer full AST serialization so OR / prefix / NOT round-trip in Has the words.
  const hasWords = parsed.ftsAst
    ? serializeFtsAst(parsed.ftsAst)
    : [
        ...parsed.terms,
        ...parsed.phrases.map((p) => quoteToken(p)),
      ].join(" ");
  const doesntHave = parsed.ftsAst
    ? undefined
    : parsed.exclude.map((e) => quoteToken(e)).join(" ") || undefined;
  const attachmentFilter: AttachmentFilter =
    parsed.hasAttachment === true
      ? "yes"
      : parsed.hasAttachment === false
        ? "no"
        : "any";

  return {
    mode: parsed.mode,
    within: parsed.within ?? undefined,
    // Combined Handle stays on handle:; split fields stay on first/last/phone.
    handle: parsed.handle ?? undefined,
    firstName: parsed.firstName ?? undefined,
    lastName: parsed.lastName ?? undefined,
    phone: parsed.phone ?? undefined,
    noFirstName: parsed.noFirstName || undefined,
    noLastName: parsed.noLastName || undefined,
    withPerson: parsed.with ?? undefined,
    fromPerson: parsed.from ?? undefined,
    toPerson: parsed.to ?? undefined,
    hasWords: hasWords || undefined,
    doesntHave,
    subject: parsed.subject ?? undefined,
    filename: parsed.filename ?? undefined,
    filetype: parsed.filetype ?? undefined,
    larger:
      parsed.largerBytes != null
        ? formatSizeBytes(parsed.largerBytes)
        : undefined,
    smaller:
      parsed.smallerBytes != null
        ? formatSizeBytes(parsed.smallerBytes)
        : undefined,
    inConversation: parsed.inConversation ?? undefined,
    groupBy: parsed.groupBy,
    context: parsed.context > 0 ? parsed.context : undefined,
    sort: parsed.sort,
    date: dateFilterFromBounds(parsed.after, parsed.before),
    firstContact: dateFilterFromBounds(
      parsed.firstContact.from,
      parsed.firstContact.to,
    ),
    lastContact: dateFilterFromBounds(
      parsed.lastContact.from,
      parsed.lastContact.to,
    ),
    groupCount: parsed.groupCount
      ? {
          comparator: parsed.groupCount.comparator,
          value: String(parsed.groupCount.value),
        }
      : { comparator: "any", value: "" },
    messageCount: parsed.messageCount
      ? {
          comparator: parsed.messageCount.comparator,
          value: String(parsed.messageCount.value),
        }
      : { comparator: "any", value: "" },
    conversationType: parsed.conversationType ?? "any",
    source: parsed.source ?? undefined,
    attachmentFilter,
    hasAttachment: parsed.hasAttachment === true,
  };
}

/** Build a shareable query string from the advanced form fields. */
export function composeSearchQuery(form: AdvancedSearchForm): string {
  const parts: string[] = [];
  const quoteIfNeeded = (v: string) =>
    /\s/.test(v) ? `"${v.replace(/"/g, "")}"` : v;

  if (form.mode === "contacts") parts.push("search:contacts");
  if (form.within?.trim()) {
    parts.push(`within:${quoteIfNeeded(form.within.trim())}`);
  }
  // first/last/phone apply in Contacts search and Messages “with person” expand.
  if (form.noFirstName) parts.push("is:nofirst");
  else if (form.firstName?.trim()) {
    parts.push(`first:${quoteIfNeeded(form.firstName.trim())}`);
  }
  if (form.noLastName) parts.push("is:nolast");
  else if (form.lastName?.trim()) {
    parts.push(`last:${quoteIfNeeded(form.lastName.trim())}`);
  }
  if (form.phone?.trim()) {
    parts.push(`phone:${quoteIfNeeded(form.phone.trim())}`);
  }
  if (form.mode === "contacts" && form.handle?.trim()) {
    parts.push(`handle:${quoteIfNeeded(form.handle.trim())}`);
  }
  if (form.mode !== "contacts" && form.fromPerson?.trim()) {
    parts.push(`from:${quoteIfNeeded(form.fromPerson.trim())}`);
  }
  if (form.mode !== "contacts" && form.toPerson?.trim()) {
    parts.push(`to:${quoteIfNeeded(form.toPerson.trim())}`);
  }
  if (form.mode !== "contacts" && form.withPerson?.trim()) {
    parts.push(`with:${quoteIfNeeded(form.withPerson.trim())}`);
  }
  if (form.mode !== "contacts" && form.inConversation?.trim()) {
    parts.push(`in:${quoteIfNeeded(form.inConversation.trim())}`);
  }
  if (form.mode !== "contacts" && form.subject?.trim()) {
    parts.push(`subject:${quoteIfNeeded(form.subject.trim())}`);
  }
  if (form.mode !== "contacts" && form.hasWords?.trim()) {
    parts.push(form.hasWords.trim());
  }
  if (form.mode !== "contacts" && form.doesntHave?.trim()) {
    for (const t of tokenize(form.doesntHave)) {
      if (t.startsWith('"')) parts.push(`-${t}`);
      else parts.push(`-${t.replace(/^-/, "")}`);
    }
  }

  const date = form.mode !== "contacts" ? form.date : undefined;
  if (date && date.mode !== "any") {
    const from = date.from?.trim();
    const to = date.to?.trim();
    if (date.mode !== "before" && from) parts.push(`after:${from}`);
    if (date.mode !== "on-or-after" && to) parts.push(`before:${to}`);
  }

  const firstContact =
    form.mode === "contacts" ? composeDateBounds(form.firstContact) : null;
  if (firstContact) parts.push(`first-contact:${firstContact}`);
  const lastContact =
    form.mode === "contacts" ? composeDateBounds(form.lastContact) : null;
  if (lastContact) parts.push(`last-contact:${lastContact}`);

  const groupCount =
    form.mode === "contacts" ? composeCountComparison(form.groupCount) : null;
  if (groupCount) parts.push(`group-count:${groupCount}`);
  const messageCount =
    form.mode === "contacts" ? composeCountComparison(form.messageCount) : null;
  if (messageCount) parts.push(`message-count:${messageCount}`);

  if (form.mode !== "contacts") {
    if (form.conversationType === "group") parts.push("is:group");
    if (form.conversationType === "individual") parts.push("is:direct");
    if (form.source?.trim()) parts.push(`source:${form.source.trim()}`);
    const attachment =
      form.attachmentFilter ??
      (form.hasAttachment ? "yes" : "any");
    if (attachment === "yes") parts.push("has:attachment");
    if (attachment === "no") parts.push("has:noattachment");
    if (form.filename?.trim()) {
      parts.push(`filename:${quoteIfNeeded(form.filename.trim())}`);
    }
    if (form.filetype?.trim() && form.filetype !== "any") {
      parts.push(`filetype:${quoteIfNeeded(form.filetype.trim())}`);
    }
    if (form.larger?.trim() && parseSizeBytes(form.larger) != null) {
      parts.push(`larger:${form.larger.trim()}`);
    }
    if (form.smaller?.trim() && parseSizeBytes(form.smaller) != null) {
      parts.push(`smaller:${form.smaller.trim()}`);
    }
    if (form.groupBy === "none") parts.push("group:none");
    if (form.context != null && form.context > 0) {
      parts.push(`context:${form.context}`);
    }
    if (form.sort && form.sort !== "date-desc") {
      parts.push(`sort:${form.sort}`);
    }
  }
  return parts.join(" ");
}

export function hasSearchCriteria(q: ParsedSearchQuery): boolean {
  return (
    !!q.ftsAst ||
    q.terms.length > 0 ||
    q.phrases.length > 0 ||
    q.exclude.length > 0 ||
    q.mode === "contacts" ||
    !!q.from ||
    !!q.to ||
    !!q.with ||
    !!q.subject ||
    !!q.text ||
    q.hasAttachment !== null ||
    !!q.filename ||
    !!q.filetype ||
    q.largerBytes != null ||
    q.smallerBytes != null ||
    !!q.inConversation ||
    !!q.after ||
    !!q.before ||
    !!q.source ||
    !!q.conversationType ||
    !!q.within ||
    !!q.handle ||
    !!q.firstName ||
    !!q.lastName ||
    !!q.phone ||
    q.noFirstName ||
    q.noLastName ||
    !!q.groupCount ||
    !!q.messageCount ||
    hasDateBounds(q.lastContact) ||
    hasDateBounds(q.firstContact)
  );
}

/**
 * Convert parsed free-text / phrases into an FTS5 MATCH expression.
 * Returns null when there is nothing to match via FTS.
 */
export function toFtsMatch(q: ParsedSearchQuery): string | null {
  const parts: string[] = [];
  if (q.ftsAst) {
    const compiled = compileFtsNode(q.ftsAst);
    if (compiled) parts.push(compiled);
  }
  if (q.subject?.trim()) {
    const safe = q.subject.trim().replace(/"/g, "");
    if (safe) parts.push(`subject:"${safe}"`);
  }
  if (q.text?.trim()) {
    const safe = q.text.trim().replace(/"/g, "");
    if (safe) parts.push(`body:"${safe}"`);
  }
  if (parts.length === 0) return null;
  if (parts.length === 1) return parts[0]!;
  return parts.join(" AND ");
}
