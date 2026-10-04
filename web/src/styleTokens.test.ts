import { readdirSync, readFileSync } from "node:fs";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { RANGE_PILL_SCROLL_PAD_CLASS, RangePillSpacer } from "./components/ListRangePill";
import { AVATAR_COLOR_CLASSES } from "./lib/contactInitials";
import { focusOutline, focusRing } from "./lib/uiStyles";
import { Z_CONTACT_DRAWER, Z_DRAWER_SCRIM, Z_MODAL, Z_RESIZE_HANDLE } from "./lib/zLayers";

// The style guide's rules (STYLE_GUIDE.md, "Rules" 1 and "Overlay Z-Index
// Ladder"): colors come from theme.css tokens so the light and dark themes stay
// in one place, and z-index values come from the ladder in lib/zLayers.ts so
// overlays stack the same way everywhere.

const SRC = new URL("./", import.meta.url);
const themeCss = readFileSync(new URL("theme.css", SRC), "utf8");

/** Every .ts/.tsx source under src/, tests left out, as [path, text]. */
function sources(): [string, string][] {
  return readdirSync(SRC, { recursive: true, encoding: "utf8" })
    .filter((p) => /\.tsx?$/.test(p) && !/\.(test|spec)\.tsx?$/.test(p))
    .map((p) => [p.replaceAll("\\", "/"), readFileSync(new URL(p, SRC), "utf8")]);
}

/** Lines of `text` that `test` matches (a pattern, or a check that returns true), as "path:line: text". */
function hits(path: string, text: string, test: RegExp | ((line: string) => boolean)): string[] {
  const matches = typeof test === "function" ? test : (line: string) => test.test(line);
  return text
    .split("\n")
    .flatMap((line, i) => (matches(line) ? [`${path}:${i + 1}: ${line.trim()}`] : []));
}

// The theme presets and the color picker hold colors as data a person picks,
// not as styling, so they are the only places a hex value may appear.
const HEX_DATA_FILES = new Set(["lib/theme.ts", "components/theme/ThemeColorRow.tsx"]);

describe("colors are theme tokens", () => {
  it("no component writes a hex color, an rgb() color or a palette color", () => {
    // A hex color in code counts wherever a value can start. A comment names
    // issues as `(#1245)`, so there a short hex counts only when it holds a
    // letter, and a long one always does.
    const hexInCode = /(^|["'`[\s(,:])#([0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})\b/;
    const hexInComment =
      /(^|["'`[\s(,:])#([0-9a-fA-F]{6}|[0-9a-fA-F]{8}|(?=\d*[a-fA-F])[0-9a-fA-F]{3,4})\b/;
    const comment = /^\s*(\/\/|\/\*|\*)/;
    const hexHits = (path: string, text: string) =>
      text
        .split("\n")
        .flatMap((line, i) =>
          (comment.test(line) ? hexInComment : hexInCode).test(line)
            ? [`${path}:${i + 1}: ${line.trim()}`]
            : [],
        );
    const rgb = /(?<![a-zA-Z])(rgba?|hsla?)\(/;
    const palette =
      /\b(bg|text|border|ring|outline|fill|stroke|shadow|from|to|via|decoration)-(white|black|(slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)-\d{2,3})\b/;
    const found = sources().flatMap(([path, text]) => [
      ...(HEX_DATA_FILES.has(path) ? [] : hexHits(path, text)),
      ...hits(path, text, rgb),
      ...hits(path, text, palette),
    ]);
    expect(found).toEqual([]);
  });

  it("every token the Tailwind map names is defined", () => {
    const themeBlock = themeCss.slice(themeCss.indexOf("@theme inline"));
    const referenced = [...themeBlock.matchAll(/--(?:color|shadow)-[\w-]+:\s*var\((--[\w-]+)\)/g)]
      .map((m) => m[1])
      .filter((name): name is string => name != null);
    expect(referenced.length).toBeGreaterThan(0);
    const missing = referenced.filter((name) => !themeCss.includes(`  ${name}:`));
    expect(missing).toEqual([]);
  });

  // A black shadow tuned for the light theme all but disappears on the dark
  // theme's dark surfaces, so the dark theme sets each shadow itself (#1525).
  it("the dark theme sets every shadow itself", () => {
    const block = (selector: string) => {
      const start = themeCss.indexOf(`${selector} {`);
      return start < 0 ? "" : themeCss.slice(start, themeCss.indexOf("\n}", start));
    };
    const shadows = (css: string) =>
      [...css.matchAll(/^\s+(--elevation-[\w-]+):/gm)].map((m) => m[1]).sort();
    const root = shadows(block(":root"));
    expect(root.length).toBeGreaterThan(0);
    expect(shadows(block('[data-theme="dark"]'))).toEqual(root);
  });

  it("every avatar color class has a token", () => {
    for (const cls of AVATAR_COLOR_CLASSES) {
      expect(themeCss).toContain(`--color-${cls.replace(/^bg-/, "")}:`);
    }
  });
});

describe("focus rings", () => {
  // A comment line is blanked, not dropped, so the line numbers stay right. A
  // line opening with `*` counts only when a space, `/` or the line's end
  // follows, so a class line opening with Tailwind's `*:` variant is still read.
  const comment = /^\s*(\/\/|\/\*|\*(\s|\/|$))/;
  const code = (text: string) =>
    text
      .split("\n")
      .map((line) => (comment.test(line) ? "" : line))
      .join("\n");

  // A ring offset is a box-shadow in a colour of its own, white unless a class
  // sets it, so it drew a white line round every focused button in the dark
  // theme (#1703). `focusRing` leaves the gap as an outline offset, which shows
  // whatever surface the element sits on.
  it("no source puts an offset on a ring", () => {
    const found = sources().flatMap(([path, text]) => hits(path, text, /\bring-offset-/));
    expect(found).toEqual([]);
  });

  // Every focused outline is the same because every one comes from
  // lib/uiStyles.ts (#1711): `focusRing`, or `focusOutline` behind React Aria's
  // `isFocusVisible` render prop where focus sits on a hidden input. In the .ts
  // and .tsx sources, outside comments, these count: an outline class other
  // than `outline-none` and `outline-hidden`, which take an outline away; the
  // bare `outline` class, with or without a variant; an arbitrary
  // `[outline:…]` property; and an inline outline style with a string or
  // number value, `outline: none` aside.
  it("no source outside lib/uiStyles.ts writes an outline of its own", () => {
    const outline = new RegExp(
      [
        /\boutline-(?!(none|hidden)\b)[\w[]/.source,
        /(?<![\w-])outline(?=["'`\s]|$)/.source,
        /\boutline(Width|Style|Color|Offset)?\s*:\s*(?!["']?none\b)["'`\d]/.source,
        /\.style\.outline/.source,
      ].join("|"),
    );
    const found = sources()
      .filter(([path]) => path !== "lib/uiStyles.ts")
      .flatMap(([path, text]) => hits(path, code(text), outline));
    expect(found).toEqual([]);
  });

  // A ring drawn flush against the element on focus was a third focus style
  // beside `focusRing` and the inset ring (#1717). Outside lib/uiStyles.ts a
  // ring is the style guide's inset ring, `ring-2 ring-inset ring-accent`, for
  // an element that draws its ring inside itself (a table row, a resize grip).
  // The check reads one line at a time, so a ring's classes go on one line.
  // On each line, the ring classes are grouped by variant (`!` aside). A group
  // whose variant names focus (`focus-visible:`, `data-focus-visible:`,
  // `has-[…:focus-visible]:` and the rest), and a group with no variant, which
  // a render prop such as `isFocused` may switch on whatever its name, must
  // have `ring-inset` under its variant or bare, and a width of 2 only.
  // `ring-0` takes a ring away, so it is no width. A bare `ring` is a 1px
  // width under a variant or beside another ring class, and otherwise the
  // word in a sentence. Tailwind's `inset-ring-*` is a second inset ring, so
  // it counts on any line. RING_HALOS holds the exact classes of each ring
  // that is not a focus ring.
  const RING_HALOS = new Map([["components/StepProgress.tsx", "ring-4 ring-accent/30"]]);
  const ringClass = /^((?:[^:]*:)*)!?(inset-)?ring(?:-(.+?))?!?$/;
  /** A token with the brackets of the code around it taken off, its own kept. */
  const trim = (token: string) => {
    let t = token;
    const count = (c: string) => t.split(c).length - 1;
    while (t.startsWith("(") && count("(") > count(")")) t = t.slice(1);
    while (t.endsWith(")") && count(")") > count("(")) t = t.slice(0, -1);
    return t.replace(/[,;]+$/, "");
  };
  const isWidth = (rest: string) =>
    rest === "" || /^(\d+|\[\d+(\.\d+)?(px|rem|em)?\]|\((length:)?--[\w-]+\))$/.test(rest);
  const flushRing = (path: string) => (line: string) => {
    const halo = RING_HALOS.get(path);
    const text = halo !== undefined ? line.replace(halo, "") : line;
    const byVariant = new Map<string, string[]>();
    for (const token of text.split(/[\s"'`{}$+?]+/).map(trim)) {
      const m = ringClass.exec(token);
      if (!m) continue;
      if (m[2] !== undefined) return true;
      const variant = m[1] ?? "";
      byVariant.set(variant, [...(byVariant.get(variant) ?? []), m[3] ?? ""]);
    }
    const bare = byVariant.get("");
    if (bare?.every((rest) => rest === "")) byVariant.delete("");
    const inset = (variant: string) =>
      (byVariant.get(variant) ?? []).includes("inset") ||
      (byVariant.get("") ?? []).includes("inset");
    return [...byVariant].some(([variant, rests]) => {
      if (variant !== "" && !variant.includes("focus")) return false;
      const ring = rests.filter(
        (rest) => rest !== "inset" && rest !== "0" && !rest.startsWith("offset-"),
      );
      if (ring.length === 0) return false;
      return !inset(variant) || ring.filter(isWidth).some((width) => width !== "2");
    });
  };
  it("no source outside lib/uiStyles.ts draws a focus ring other than the inset ring", () => {
    const found = sources()
      .filter(([path]) => path !== "lib/uiStyles.ts")
      .flatMap(([path, text]) => hits(path, code(text), flushRing(path)));
    expect(found).toEqual([]);
  });

  it("focusRing is focusOutline on focus-visible", () => {
    const onFocusVisible = focusOutline
      .split(" ")
      .map((cls) => `focus-visible:${cls}`)
      .join(" ");
    expect(focusRing).toBe(`outline-none ${onFocusVisible}`);
  });
});

describe("z-index values come from the ladder", () => {
  it("no source outside lib/zLayers.ts writes a z-index class", () => {
    const zClass = /(^|[\s"'`:!])-?z-(\d|\[)/;
    const found = sources()
      .filter(([path]) => path !== "lib/zLayers.ts")
      .flatMap(([path, text]) => hits(path, text, zClass));
    expect(found).toEqual([]);
  });

  it("no source sets a z-index inline", () => {
    const found = sources().flatMap(([path, text]) => hits(path, text, /\bzIndex\b|z-index\s*:/));
    expect(found).toEqual([]);
  });

  // The overlay contact drawer is not modal and stays open while Sources or a
  // dialog opens; under the scrim, that dialog dims it and takes its clicks.
  it("the overlay contact drawer sits above the resize handles and below every scrim", () => {
    const rung = (cls: string) => Number(/^z-\[?(\d+)\]?$/.exec(cls)?.[1]);
    expect(rung(Z_CONTACT_DRAWER)).toBeGreaterThan(rung(Z_RESIZE_HANDLE));
    expect(rung(Z_CONTACT_DRAWER)).toBeLessThan(rung(Z_DRAWER_SCRIM));
    expect(rung(Z_CONTACT_DRAWER)).toBeLessThan(rung(Z_MODAL));
  });
});

describe("the range pill's room", () => {
  // A list leaves room under its last row either as padding or as a spacer;
  // the two must match, or one kind of list hides its last row under the pill.
  it("is the same size as padding and as a spacer", () => {
    const size = RANGE_PILL_SCROLL_PAD_CLASS.replace(/^pb-/, "");
    const markup = renderToStaticMarkup(createElement(RangePillSpacer));
    const classes = /class="([^"]*)"/.exec(markup)?.[1]?.split(" ") ?? [];
    expect(classes).toContain(`h-${size}`);
  });
});
