import { lazyChunk } from "../lib/chunkLoad";

/**
 * Settings, whose code is downloaded the first time it is shown, from its own
 * route or from Owner Home. Both load it through this one declaration: a
 * static import anywhere would put Settings back into the first page's code.
 */
export const SettingsScreen = lazyChunk("SettingsScreen", () => import("./SettingsScreen"));
