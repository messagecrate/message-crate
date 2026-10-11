// What the app layout and the routes under it share: the section each route
// declares for the header and the navigation panel, the navigation panel's
// item each route is on, and the contact panel the layout holds for them.

import { createContext, useCallback, useContext, useLayoutEffect, useRef, useState } from "react";
import type { HeaderSearch } from "./AppHeader";
import type { ContactBrowseTarget, ContactPreview } from "./contactDrawer/contactDrawerTypes";

/**
 * The navigation panel's item a route is on, which the panel highlights: one
 * of its Browse rows, or Import or Export. The route declares it, so the
 * panel needs no route table of its own to find these rows' current one.
 */
export type NavItem = "messages" | "contacts" | "trash" | "import" | "export";

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
  /** The navigation panel's item the route is on. */
  navItem: NavItem;
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
  /** Declare the navigation panel's item the route is on, or null for none. */
  declareNavItem: (item: NavItem | null) => void;
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
  browseContactConversations: (target: ContactBrowseTarget) => void;
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
 * The layout's side of the sections: the section shown, the navigation
 * panel's item, the functions a route declares them with, and the header's
 * two callbacks. The callbacks are read from the newest declaration when
 * called, so they always act on the route's address as it is now, and a
 * route that renders again with the same words does not render the layout
 * again.
 */
export function useSectionSlot(): {
  shown: ShownSection | null;
  declareSection: (section: LayoutSection | null) => void;
  navItem: NavItem | null;
  declareNavItem: (item: NavItem | null) => void;
  onSearchChange: (q: string) => void;
  onSearch: (q: string) => void;
} {
  const latest = useRef<LayoutSection | null>(null);
  const [shown, setShown] = useState<ShownSection | null>(null);
  const [navItem, declareNavItem] = useState<NavItem | null>(null);
  const declareSection = useCallback((section: LayoutSection | null) => {
    latest.current = section;
    const next = section && { search: section.search, browseQuery: section.browseQuery };
    setShown((prev) => (sameShown(prev, next) ? prev : next));
  }, []);
  const onSearchChange = useCallback((q: string) => latest.current?.onSearchChange(q), []);
  const onSearch = useCallback((q: string) => latest.current?.onSearch(q), []);
  return { shown, declareSection, navItem, declareNavItem, onSearchChange, onSearch };
}

/**
 * Declare the navigation panel's item this route is on, or null for a route
 * the panel has no item for, such as Settings. A route with a section
 * declares its item through `useLayoutSection`; a route with no list, such as
 * Import, declares it here. Set before the browser paints, so the panel never
 * highlights the item of the route before for a frame.
 */
export function useLayoutNavItem(item: NavItem | null): void {
  const { declareNavItem } = useAppLayout();
  useLayoutEffect(() => {
    declareNavItem(item);
    // Leaving the route takes its item with it.
    return () => declareNavItem(null);
  }, [declareNavItem, item]);
}

/**
 * Declare this route's section to the layout. Set before the browser paints,
 * so the header never shows the section of the route before for a frame.
 */
export function useLayoutSection(section: LayoutSection): void {
  const { declareSection } = useAppLayout();
  useLayoutNavItem(section.navItem);
  // Every render, so the layout's callbacks see this render's address.
  useLayoutEffect(() => {
    declareSection(section);
  });
  // Leaving the route takes its section with it.
  useLayoutEffect(() => () => declareSection(null), [declareSection]);
}
