import type { ReactNode } from "react";
import type { Key } from "react-aria-components";
import { Tab as RACTab, SelectionIndicator } from "react-aria-components";
import { focusRing } from "../lib/uiStyles";

/**
 * The app's tab: React Aria's `Tab`, drawn as a label over an accent line that
 * slides to the selected tab. Settings and the login card both use it, so a
 * change to how a tab looks is made here once (#1728).
 *
 * The tone comes from React Aria's data attributes: muted, the text colour when
 * hovered or selected, and dimmed with the not-allowed cursor when disabled
 * (`web/STYLE_GUIDE.md`, "Disabled"). React Aria never marks a disabled tab
 * hovered, so a disabled tab keeps its muted tone under the pointer.
 *
 * Its `TabList` draws the `border-b border-border` rule; the tab's `-mb-px`
 * lays the accent line over it.
 */
export type TabProps = {
  id: Key;
  children: ReactNode;
  /** The text size and layout of the strip it sits in, such as `flex-1 text-center`. */
  className?: string;
};

const tabClass = `relative -mb-px cursor-pointer border-none bg-transparent px-3 py-2 font-medium text-muted transition-colors duration-200 data-hovered:text-text data-selected:text-text data-disabled:cursor-not-allowed data-disabled:opacity-50 ${focusRing}`;

export default function Tab({ id, children, className = "" }: TabProps) {
  return (
    <RACTab id={id} className={`${tabClass} ${className}`}>
      {children}
      <SelectionIndicator className="absolute bottom-0 left-2 right-2 h-[2px] rounded-full bg-accent transition-[translate,width] duration-200 motion-reduce:transition-none" />
    </RACTab>
  );
}
