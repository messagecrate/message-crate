import type { ReactNode } from "react";
import { Column, Group } from "react-aria-components";
import { mutedClass, thClass, thLeftClass, thRightClass } from "./handleTableStyles";

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
  const justify =
    align === "right" ? "justify-end" : align === "left" ? "justify-start" : "justify-center";
  const textAlign =
    align === "right" ? "text-right" : align === "left" ? "text-left" : "text-center";
  const headerAlign = align === "right" ? thRightClass : align === "left" ? thLeftClass : thClass;

  return (
    <Column
      id={id}
      isRowHeader={isRowHeader}
      allowsSorting
      className={`${headerAlign} ${widthClass}`.trim()}
    >
      {({ sortDirection }) => (
        <div className="relative flex w-full min-w-0 items-center">
          <Group
            className={`flex min-w-0 flex-1 items-center outline-none ${justify} ${
              align === "right" ? "pr-4" : align === "left" ? "" : "px-4"
            }`}
          >
            <span
              className={`max-w-full leading-tight ${textAlign} ${
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
