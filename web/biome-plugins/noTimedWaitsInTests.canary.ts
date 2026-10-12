// Every form noTimedWaitsInTests.grit must catch, each with its error
// suppressed. A form the plugin stops catching (a Biome upgrade, a broken
// pattern) leaves its suppression unused, which Biome reports as a warning;
// check-pr.sh and CI's web job lint this file with --error-on-warnings, so
// that warning fails them. Biome reports a plugin that fails to run only as
// an info, which is why this file exists.
import { screen, waitFor, waitForElementToBeRemoved, within } from "@testing-library/react";
import { vi } from "vitest";

export async function everyTimedWait(element: HTMLElement): Promise<void> {
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for waitFor
  await waitFor(() => element);
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for vi.waitFor
  await vi.waitFor(() => element);
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for vi.waitUntil
  await vi.waitUntil(() => element);
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for waitForElementToBeRemoved
  await waitForElementToBeRemoved(element);
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for screen.findBy
  await screen.findByRole("button");
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for screen.findAllBy
  await screen.findAllByText("text");
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for within().findBy
  await within(element).findByLabelText("label");
  const { findByText } = within(element);
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for a destructured findBy
  await findByText("text");
}

export async function everyRealTimer(): Promise<void> {
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for setTimeout
  await new Promise((resolve) => setTimeout(resolve, 0));
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for window.setTimeout
  window.setTimeout(() => {}, 0);
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for setInterval
  const id = setInterval(() => {}, 1);
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for globalThis.setInterval
  globalThis.setInterval(() => {}, 1);
  clearInterval(id);
}
