import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// The website's tab icon (#3014). A browser asks for /favicon.ico on every
// page load unless index.html names an icon, and the server answers 404 for a
// file that public/ does not hold, which the browser logs as a console error.
// Vite copies public/ to the root of the build, so each icon index.html links
// must be a file in public/.

const WEB = new URL("../", import.meta.url);
const indexHtml = readFileSync(new URL("index.html", WEB), "utf8");

/** The href of every `<link rel="icon">` in index.html. */
function iconHrefs(): string[] {
  return [...indexHtml.matchAll(/<link\b[^>]*>/g)]
    .map(([tag]) => tag)
    .filter((tag) => /\brel="icon"/.test(tag))
    .map((tag) => /\bhref="([^"]*)"/.exec(tag)?.[1] ?? "");
}

describe("favicon", () => {
  it("links the SVG icon and the ICO icon from index.html", () => {
    expect(iconHrefs()).toEqual(["/favicon.ico", "/favicon.svg"]);
  });

  it("serves every linked icon from public/", () => {
    for (const href of iconHrefs()) {
      expect(existsSync(new URL(`public${href}`, WEB)), href).toBe(true);
    }
  });

  it("keeps /favicon.ico in public/, which a browser asks for unprompted", () => {
    expect(existsSync(new URL("public/favicon.ico", WEB))).toBe(true);
  });
});
