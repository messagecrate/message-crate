import { useCallback, useRef, useState } from "react";
import type { LayoutSection, NavItem, ShownSection } from "./appLayoutContext";

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
