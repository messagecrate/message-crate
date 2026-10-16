// Every form web/biome-plugins/noTauriMocksOutsideDesktop.grit must catch,
// each with its error suppressed. A form the plugin stops catching (a Biome
// upgrade, a broken pattern, a narrowed glob in web/biome.json) leaves its
// suppression unused, which Biome reports as a warning; check-pr.sh and CI's
// web job lint this file with --error-on-warnings, so that warning fails them.
// Biome reports a plugin that fails to run only as an info, which is why this
// file exists. Nothing imports it and Vitest doesn't run it: it is only
// linted.
import { vi } from "vitest";

export async function everyTauriMock(): Promise<void> {
  // biome-ignore lint/plugin/noTauriMocksOutsideDesktop: canary for vi.mock with a factory
  vi.mock("@tauri-apps/api/core", () => ({}));
  // biome-ignore lint/plugin/noTauriMocksOutsideDesktop: canary for vi.mock without a factory
  vi.mock("@tauri-apps/api/window");
  // biome-ignore lint/plugin/noTauriMocksOutsideDesktop: canary for vi.doMock
  vi.doMock("@tauri-apps/plugin-dialog", async () => ({}));
  // biome-ignore lint/plugin/noTauriMocksOutsideDesktop: canary for vi.unmock in a template literal
  vi.unmock(`@tauri-apps/api/event`);
  // biome-ignore lint/plugin/noTauriMocksOutsideDesktop: canary for vi.doUnmock
  vi.doUnmock("@tauri-apps/api/event");
  // biome-ignore lint/plugin/noTauriMocksOutsideDesktop: canary for vi.importActual
  await vi.importActual("@tauri-apps/api/core");
  // biome-ignore lint/plugin/noTauriMocksOutsideDesktop: canary for vi.importMock
  await vi.importMock("@tauri-apps/api/core");
}
