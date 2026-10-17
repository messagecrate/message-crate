import type { ReactNode } from "react";
import { Column, Group } from "react-aria-components";
import { mutedClass, thClass, thLeftClass, thRightClass } from "./handleTableStyles";

/** The classes a sortable column takes for each alignment: header cell, label row, its padding, and label text. */
const COLUMN_ALIGN_CLASSES = {
  left: {
    headerClass: thLeftClass,
    justifyClass: "justify-start",
    padClass: "",
    textAlignClass: "text-left",
  },
  center: {
    headerClass: thClass,
    justifyClass: "justify-center",
    padClass: "px-4",
    textAlignClass: "text-center",
  },
  right: {
    headerClass: thRightClass,
    justifyClass: "justify-end",
    padClass: "pr-4",
    textAlignClass: "text-right",
  },
};

export function SortableColumn({
  id,
  widthClass = "",
  align = "center",
  isRowHeader,
  children,
}: {
  id: string;
  widthClass?: string;
  align?: "left" | "center" | "right";
  isRowHeader?: boolean;
  children: ReactNode;
}) {
  const { headerClass, justifyClass, padClass, textAlignClass } = COLUMN_ALIGN_CLASSES[align];

  return (
    <Column
      id={id}
      isRowHeader={isRowHeader}
      allowsSorting
      className={`${headerClass} ${widthClass}`.trim()}
    >
      {({ sortDirection }) => (
        <div className="relative flex w-full min-w-0 items-center">
          <Group
            className={`flex min-w-0 flex-1 items-center outline-none ${justifyClass} ${padClass}`}
          >
            <span
              className={`max-w-full leading-tight ${textAlignClass} ${
                sortDirection ? "text-accent" : "text-text"
              }`}
            >
              {children}
            </span>
          </Group>
          <span
            aria-hidden="true"
            className={`pointer-events-none absolute top-1/2 right-1 -translate-y-1/2 text-[0.55rem] leading-none ${
              sortDirection ? "text-accent" : "invisible"
            }`}
          >
            {sortDirection === "descending" ? "▼" : "▲"}
          </span>
        </div>
      )}
    </Column>
  );
}

export function CountCell({
  value,
  loading = false,
}: {
  value: number;
  /** When true, show an em dash instead of a zeroed stub count. */
  loading?: boolean;
}) {
  if (loading) {
    return <span className={mutedClass}>—</span>;
  }
  return <span className={value === 0 ? mutedClass : undefined}>{value.toLocaleString()}</span>;
}
