/**
 * A stand-in for `IntersectionObserver`, which jsdom lacks, so a test can say
 * when an element scrolls near the screen.
 *
 * `installIntersectionObserver` puts it on `globalThis` for the test file;
 * `scrollNear(element)` tells every observer watching that element, or an
 * element inside it, that it now intersects, as the browser does when the
 * element comes within the observer's margin.
 */

import { act } from "@testing-library/react";

type Watcher = {
  root: Element | null;
  rootMargin: string;
  callback: IntersectionObserverCallback;
  observer: IntersectionObserver;
  targets: Set<Element>;
};

const watchers = new Set<Watcher>();

class FakeIntersectionObserver {
  readonly root: Element | null;
  readonly rootMargin: string;
  readonly thresholds: number[] = [0];
  private readonly watcher: Watcher;

  constructor(callback: IntersectionObserverCallback, options: IntersectionObserverInit = {}) {
    this.root = (options.root as Element | null) ?? null;
    this.rootMargin = options.rootMargin ?? "0px";
    this.watcher = {
      root: this.root,
      rootMargin: this.rootMargin,
      callback,
      observer: this as unknown as IntersectionObserver,
      targets: new Set(),
    };
    watchers.add(this.watcher);
  }

  observe(target: Element) {
    this.watcher.targets.add(target);
  }

  unobserve(target: Element) {
    this.watcher.targets.delete(target);
  }

  disconnect() {
    this.watcher.targets.clear();
    watchers.delete(this.watcher);
  }

  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }
}

/** Put the stand-in on `globalThis`, and forget every observer a test left behind. */
export function installIntersectionObserver(): void {
  watchers.clear();
  globalThis.IntersectionObserver =
    FakeIntersectionObserver as unknown as typeof IntersectionObserver;
}

/** Tell the observers watching `element`, or anything inside it, that it has come near the screen. */
export function scrollNear(element: Element): void {
  act(() => {
    for (const watcher of [...watchers]) {
      const hit = [...watcher.targets].filter((t) => element === t || element.contains(t));
      if (hit.length === 0) continue;
      watcher.callback(
        hit.map(
          (target) =>
            ({ target, isIntersecting: true, intersectionRatio: 1 }) as IntersectionObserverEntry,
        ),
        watcher.observer,
      );
    }
  });
}

/** The root and margin of each observer watching something inside `element`. */
export function observersWithin(element: Element): { root: Element | null; rootMargin: string }[] {
  return [...watchers]
    .filter((w) => [...w.targets].some((t) => element === t || element.contains(t)))
    .map(({ root, rootMargin }) => ({ root, rootMargin }));
}
