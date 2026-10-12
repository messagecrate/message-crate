// Every form web/biome-plugins/noTimedWaitsInTests.grit must catch, each with
// its error suppressed. A form the plugin stops catching (a Biome upgrade, a
// broken pattern, a narrowed glob in web/biome.json) leaves its suppression
// unused, which Biome reports as a warning; check-pr.sh and CI's web job lint
// this file with --error-on-warnings, so that warning fails them. Biome
// reports a plugin that fails to run only as an info, which is why this file
// exists. Nothing imports it and Vitest doesn't run it: it is only linted.
// biome-ignore lint/plugin/noTimedWaitsInTests: canary for a timers/promises sleep
import { setTimeout as sleep } from "node:timers/promises";
import {
  screen,
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for an imported waitFor, aliased
  waitFor as wf,
  within,
} from "@testing-library/react";
import { expect, vi } from "vitest";

export async function everyTimedWait(element: HTMLElement): Promise<void> {
  await wf(() => element);
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for vi.waitFor
  await vi.waitFor(() => element);
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for vi.waitUntil
  await vi.waitUntil(() => element);
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for expect.poll
  await expect.poll(() => element).toBeTruthy();
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for screen.findBy
  await screen.findByRole("button");
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for a typed findBy
  await screen.findByRole<HTMLButtonElement>("button");
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for an optional findBy call
  await screen.findByText?.("text");
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for screen.findAllBy
  await screen.findAllByText("text");
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for within().findBy
  await within(element).findByLabelText("label");
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for a destructured findBy
  const { findByTestId } = within(element);
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for a destructured findBy's call
  await findByTestId("id");
}

export async function everyRealTimer(): Promise<void> {
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for setTimeout
  await new Promise((resolve) => setTimeout(resolve, 0));
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for window.setTimeout passed on
  const later = window.setTimeout;
  later(() => {}, 0);
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for setInterval
  const id = setInterval(() => {}, 1);
  clearInterval(id);
  // biome-ignore lint/plugin/noTimedWaitsInTests: canary for requestAnimationFrame
  await new Promise((resolve) => requestAnimationFrame(resolve));
  await sleep(1);
}
