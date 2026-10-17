import { useContext, useLayoutEffect } from "react";
import {
  AppLayoutContext,
  type AppLayoutContextValue,
  type LayoutSection,
  type NavItem,
} from "./appLayoutContext";

/** The app layout's context; a route under the layout always has one. */
export function useAppLayout(): AppLayoutContextValue {
  const value = useContext(AppLayoutContext);
  if (value === null) throw new Error("useAppLayout is used outside the app layout");
  return value;
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
