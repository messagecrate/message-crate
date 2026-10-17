// What the app layout and the routes under it share: the section each route
// declares for the header and the navigation panel, the navigation panel's
// item each route is on, and the contact panel the layout holds for them.

import { createContext } from "react";
import type { ContactBrowseTarget } from "../lib/contactBrowseQuery";
import type { HeaderSearch } from "./AppHeader";
import type { ContactPreview } from "./contactDrawer/contactDrawerTypes";

/**
 * The navigation panel's item a route is on, which the panel highlights: one
 * of its Browse rows, or Import or Export. The route declares it, so the
 * panel needs no route table of its own to know which of these rows to
 * highlight.
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
