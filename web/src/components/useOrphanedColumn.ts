import { useState } from "react";
import type { SortDescriptor } from "react-aria-components";
import { hasOrphanedMessages } from "./identityRows";

/**
 * The Orphaned column of a table of identity rows: whether it shows, and the
 * table's sort as the table applies it. A sort by the Orphaned column is
 * suspended while the column is hidden, so the rows are never ordered by a
 * column the table does not show, and it returns with the column. The chosen
 * sort is kept rather than cleared because the column can go for a moment
 * while figures reload, and a reload must not undo what the person chose.
 * Deriving the applied sort from the rows the render already has needs no
 * state update during render and no effect.
 */
export function useOrphanedColumn(rows: readonly ({ orphaned_messages: number } | null)[]) {
  const [sort, setSort] = useState<SortDescriptor | null>(null);
  const showOrphaned = hasOrphanedMessages(rows);
  const applied = !showOrphaned && sort?.column === "orphaned_messages" ? null : sort;
  return { showOrphaned, sort: applied, setSort };
}
