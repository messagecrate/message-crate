import type { ConversationSort, ConversationSortState } from "../lib/conversationSort";
import type { SortOrder } from "../lib/sortOrder";
import SortMenu, { type SortField } from "./SortMenu";

const FIELDS: ReadonlyArray<SortField<ConversationSort>> = [
  { id: "date", label: "Date" },
  { id: "messages", label: "Messages" },
];

export default function ConversationSortMenu({
  sort,
  order,
  onChange,
}: {
  sort: ConversationSort;
  order: SortOrder;
  onChange: (next: ConversationSortState) => void;
}) {
  return (
    <SortMenu
      fields={FIELDS}
      sort={sort}
      order={order}
      onChange={onChange}
      itemNoun="conversations"
    />
  );
}
