import { useMemo, useState } from "react";
import {
  Cell,
  Column,
  type Key,
  Row,
  Table,
  TableBody,
  TableHeader,
  TableLayout,
  Virtualizer,
} from "react-aria-components";
import { ChevronDownIcon, ChevronRightIcon } from "../icons";
import PlainButton from "../PlainButton";
import { groupImportIssues, type ImportIssueGroup } from "./groupImportIssues";
import type { ImportIssue, ImportNote } from "./ImportSummaryPanel";
import { ISSUE_STAGE_LABEL } from "./importIssueStage";
import {
  COLLAPSED_ROW_HEIGHT,
  FILENAME_ROW_PX,
  HEADER_ROW_HEIGHT,
  MAX_VISIBLE_FILENAMES,
  tableViewportHeight,
} from "./importIssuesTableLayout";

const headerClass = "min-w-0 px-3 py-2 text-left font-medium text-muted outline-none";
const cellClass = "min-w-0 overflow-hidden px-3 py-2 text-text";

/** Row height is collapsed until a row expands; React Aria measures that one row. */
const LAYOUT_OPTIONS = {
  estimatedRowHeight: COLLAPSED_ROW_HEIGHT,
  headingHeight: HEADER_ROW_HEIGHT,
};

type IssueRow = ImportIssueGroup & { id: string };

/** The words the table uses for its rows: Import Errors, or notes. */
type TableWords = {
  label: string;
  itemHeader: string;
  reasonHeader: string;
  noun: string;
  items: string;
};

const ERROR_WORDS: TableWords = {
  label: "Import errors",
  itemHeader: "Parse File",
  reasonHeader: "Error Message",
  noun: "error",
  items: "files",
};

const NOTE_WORDS: TableWords = {
  label: "Import notes",
  itemHeader: "Item",
  reasonHeader: "Note",
  noun: "note",
  items: "items",
};

function parseFileLabel(group: ImportIssueGroup, words: TableWords): string {
  if (group.items.length === 1) {
    return group.items[0] ?? "";
  }
  return `${group.items.length} ${words.items}`;
}

function rowAriaLabel(group: ImportIssueGroup, expanded: boolean, words: TableWords): string {
  const verb = expanded ? "Collapse" : "Expand";
  return `${verb} ${words.noun} for ${parseFileLabel(group, words)}`;
}

/**
 * The notes of an import, one row per distinct note, drawn the way the
 * Import Errors are.
 */
export function VirtualizedImportNotesTable({ notes }: { notes: ImportNote[] }) {
  const rows = useMemo<ImportIssue[]>(
    () => notes.map(({ stage, item, text }) => ({ kind: "note", stage, item, reason: text })),
    [notes],
  );
  return <IssuesTable issues={rows} words={NOTE_WORDS} />;
}

/**
 * The errors of an import, one row per distinct error. React Aria's virtualized
 * `Table` draws only the rows in view, moves focus between rows with the arrow
 * keys, and runs the row's action on a click or Enter, which expands the row to
 * the whole error and, for a group, its file names.
 */
export default function VirtualizedImportIssuesTable({ issues }: { issues: ImportIssue[] }) {
  return <IssuesTable issues={issues} words={ERROR_WORDS} />;
}

function IssuesTable({ issues, words }: { issues: ImportIssue[]; words: TableWords }) {
  const rows = useMemo<IssueRow[]>(
    () => groupImportIssues(issues).map((group, index) => ({ ...group, id: String(index) })),
    [issues],
  );
  const [expandedKey, setExpandedKey] = useState<Key | null>(null);
  const expandedRow = rows.find((row) => row.id === expandedKey) ?? null;
  const viewportHeight = tableViewportHeight(
    rows.length,
    expandedRow == null
      ? null
      : { reason: expandedRow.reason, fileCount: expandedRow.items.length },
  );

  const toggleRow = (key: Key) => {
    setExpandedKey((current) => (current === key ? null : key));
  };

  return (
    <div className="mt-2 w-full min-w-0 max-w-full overflow-hidden rounded-lg border border-border text-left text-[0.813rem]">
      <Virtualizer layout={TableLayout} layoutOptions={LAYOUT_OPTIONS}>
        <Table
          aria-label={words.label}
          onRowAction={toggleRow}
          className="block w-full overflow-x-hidden overflow-y-auto outline-none"
          style={{ height: HEADER_ROW_HEIGHT + viewportHeight }}
        >
          <TableHeader className="border-b border-border bg-elevated">
            <Column id="file" isRowHeader width="1fr" className={headerClass}>
              {words.itemHeader}
            </Column>
            <Column id="stage" width={72} className={headerClass}>
              Stage
            </Column>
            <Column id="reason" width="1.4fr" className={headerClass}>
              {words.reasonHeader}
            </Column>
          </TableHeader>
          <TableBody items={rows} dependencies={[expandedKey]}>
            {(row) => {
              const expanded = row.id === expandedKey;
              const fileLabel = parseFileLabel(row, words);
              return (
                <Row
                  id={row.id}
                  textValue={fileLabel}
                  className={`cursor-pointer items-start border-b border-border outline-none hover:bg-hover focus-visible:bg-hover focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent ${
                    expanded ? "bg-hover" : ""
                  }`}
                >
                  <Cell className={cellClass}>
                    <span className="flex min-w-0 items-start gap-1">
                      {/* The row is the press target; this button says the row expands, and whether it is. */}
                      <PlainButton
                        aria-label={rowAriaLabel(row, expanded, words)}
                        aria-expanded={expanded}
                        onPress={() => toggleRow(row.id)}
                        className="mt-px flex h-4 w-4 shrink-0 cursor-pointer items-center justify-center rounded-sm border-none bg-transparent p-0 text-muted outline-none hover:text-text focus-visible:ring-2 focus-visible:ring-accent"
                      >
                        {expanded ? <ChevronDownIcon size={12} /> : <ChevronRightIcon size={12} />}
                      </PlainButton>
                      <span title={fileLabel} className="block truncate">
                        {fileLabel}
                      </span>
                    </span>
                  </Cell>
                  <Cell className={`${cellClass} capitalize`}>
                    <span className="block truncate">{ISSUE_STAGE_LABEL[row.stage]}</span>
                  </Cell>
                  <Cell className={cellClass}>
                    <span
                      title={expanded ? undefined : row.reason}
                      className={
                        expanded
                          ? "block whitespace-pre-wrap break-words"
                          : "line-clamp-2 break-words"
                      }
                    >
                      {row.reason}
                    </span>
                    {expanded && row.items.length > 1 ? (
                      <ul
                        className="mt-2 overflow-y-auto text-muted"
                        style={{ maxHeight: MAX_VISIBLE_FILENAMES * FILENAME_ROW_PX }}
                        // A press on a file name selects it rather than collapsing the row.
                        onPointerDown={(event) => event.stopPropagation()}
                      >
                        {row.items.map((name, fileIndex) => (
                          <li
                            key={`${name}-${String(fileIndex)}`}
                            title={name}
                            className="truncate"
                            style={{ height: FILENAME_ROW_PX }}
                          >
                            {name}
                          </li>
                        ))}
                      </ul>
                    ) : null}
                  </Cell>
                </Row>
              );
            }}
          </TableBody>
        </Table>
      </Virtualizer>
    </div>
  );
}
