"use client";

import { isDeletionUiBlocked } from "@/lib/v1Capabilities";
import type { RefObject } from "react";
import { TrashMessagesIcon } from "./icons";

export function SearchMessageCtxMenu({
  menuRef,
  x,
  y,
  count,
  saving,
  onDelete,
}: {
  menuRef: RefObject<HTMLDivElement | null>;
  x: number;
  y: number;
  count: number;
  saving: boolean;
  onDelete: () => void;
}) {
  return (
    <div
      ref={menuRef}
      className="fixed z-[100] min-w-[180px] rounded-lg border border-border bg-popover py-1 shadow-xl"
      style={{ left: x, top: y }}
    >
      <button
        type="button"
        disabled={isDeletionUiBlocked() || saving}
        className="flex w-full items-center gap-2.5 px-3 py-1.5 text-left text-[13px] text-text hover:bg-red-500/15 hover:text-red-300 disabled:opacity-50"
        onClick={onDelete}
      >
        <TrashMessagesIcon className="size-5 shrink-0 opacity-80" />
        {count === 1 ? "Delete message" : "Delete messages"}
      </button>
    </div>
  );
}
