import { createContext, useContext, useEffect, useState } from "react";

/**
 * The element a conversation scrolls in. A message counts as near the screen
 * by its distance from this element's visible area; with none, from the
 * window's.
 */
export const NearScreenRoot = createContext<Element | null>(null);

/**
 * How far beyond the visible area an element counts as near: about a screen
 * above and below, so a Thumbnail has loaded by the time it scrolls in.
 */
export const NEAR_SCREEN_MARGIN = "800px 0px";

/**
 * Whether the element given to the returned ref has come near the screen.
 * Once it has, it stays near: what was loaded for it stays loaded.
 *
 * Without `IntersectionObserver`, which every browser the app runs in has,
 * everything counts as near.
 */
export function useNearScreen<T extends Element>(): [(node: T | null) => void, boolean] {
  const root = useContext(NearScreenRoot);
  const [node, setNode] = useState<T | null>(null);
  const [near, setNear] = useState(() => typeof IntersectionObserver === "undefined");

  useEffect(() => {
    if (near || !node) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) setNear(true);
      },
      { root, rootMargin: NEAR_SCREEN_MARGIN },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [node, root, near]);

  return [setNode, near];
}
