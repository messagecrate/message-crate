import { createContext, useCallback, useContext, useLayoutEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";
import type { HeaderSearch } from "./AppHeader";
import type { ContactBrowseKind, ContactPreview } from "./contactDrawer/contactDrawerTypes";

/**
 * What a route under the app layout tells the layout's header and navigation
 * panel. A route with a list declares one with `useLayoutSection`; a route
 * that declares none, such as Settings, has no list, so the header offers no
 * search and the navigation panel fits the window.
 */
export interface LayoutSection {
  /** The list the header search searches, and its search so far. */
  search: HeaderSearch;
  /** The search box's words changed. */
  onSearchChange: (q: string) => void;
  /** The search box was submitted. */
  onSearch: (q: string) => void;
  /** What the navigation panel's Export starts from; empty for nothing. */
  browseQuery: string;
}

/** What the layout shows from the section: the parts that are not callbacks. */
export interface ShownSection {
  search: HeaderSearch;
  browseQuery: string;
}

/** What the app layout hands the routes under it. */
export interface AppLayoutContextValue {
  /** Declare the route's section, or null when the route leaves. */
  declareSection: (section: LayoutSection | null) => void;
  /**
   * The contact open in the Contacts page's right pane. The layout holds it,
   * so it is still open when the person comes back to Contacts, and the
   * contact panel over a conversation closes it too.
   */
  selectedContact: ContactPreview | null;
  selectContact: (contact: ContactPreview | null) => void;
  /** Close the contact panel, docked or over a conversation. */
  closeContactDrawer: () => void;
  /** Open the conversations of a contact, or of one of its handles. */
  browseContactConversations: (target: {
    contactId: string;
    kind: ContactBrowseKind;
    handle?: string;
  }) => void;
}

export const AppLayoutContext = createContext<AppLayoutContextValue | null>(null);

/** The app layout's context; a route under the layout always has one. */
export function useAppLayout(): AppLayoutContextValue {
  const value = useContext(AppLayoutContext);
  if (value === null) throw new Error("useAppLayout is used outside the app layout");
  return value;
}

function sameShown(a: ShownSection | null, b: ShownSection | null): boolean {
  if (a === null || b === null) return a === b;
  return (
    a.browseQuery === b.browseQuery &&
    a.search.target === b.search.target &&
    a.search.query === b.search.query
  );
}

/**
 * The layout's side of the sections: the section shown, the function a route
 * declares its own with, and the header's two callbacks. The callbacks are
 * read from the newest declaration when called, so they always act on the
 * route's address as it is now, and a route that renders again with the same
 * words does not render the layout again.
 */
export function useSectionSlot(): {
  shown: ShownSection | null;
  declareSection: (section: LayoutSection | null) => void;
  onSearchChange: (q: string) => void;
  onSearch: (q: string) => void;
} {
  const latest = useRef<LayoutSection | null>(null);
  const [shown, setShown] = useState<ShownSection | null>(null);
  const declareSection = useCallback((section: LayoutSection | null) => {
    latest.current = section;
    const next = section && { search: section.search, browseQuery: section.browseQuery };
    setShown((prev) => (sameShown(prev, next) ? prev : next));
  }, []);
  const onSearchChange = useCallback((q: string) => latest.current?.onSearchChange(q), []);
  const onSearch = useCallback((q: string) => latest.current?.onSearch(q), []);
  return { shown, declareSection, onSearchChange, onSearch };
}

/**
 * Declare this route's section to the layout. Set before the browser paints,
 * so the header never shows the section of the route before for a frame.
 */
export function useLayoutSection(section: LayoutSection): void {
  const { declareSection } = useAppLayout();
  // Every render, so the layout's callbacks see this render's address.
  useLayoutEffect(() => {
    declareSection(section);
  });
  // Leaving the route takes its section with it.
  useLayoutEffect(() => () => declareSection(null), [declareSection]);
}

/**
 * Write `updates` into the address's query, deleting a key whose value is
 * empty. `replace: true` is deliberate: typing in a search box must not fill
 * the history with one entry per keystroke. Trash's `tsel` selection goes
 * through the same function and so is not undoable with Back, unlike
 * selecting a conversation elsewhere, which navigates.
 */
export function useReplaceSearchParams(): (updates: Record<string, string>) => void {
  const [searchParams, setSearchParams] = useSearchParams();
  return (updates) => {
    const next = new URLSearchParams(searchParams);
    for (const [k, v] of Object.entries(updates)) {
      if (v) next.set(k, v);
      else next.delete(k);
    }
    setSearchParams(next, { replace: true });
  };
}
