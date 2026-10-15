// The theme's first paint: runs before the page draws, so the page is never
// the wrong colors for a frame before React starts. It is a file of its own
// because the server's Content-Security-Policy refuses inline script
// (docs/architecture/http-api.md, "Protective headers"). The keys and
// defaults are those of src/lib/theme.ts: change the two together.
(() => {
  try {
    const mk = "mc-theme";
    const sk = "mc-theme-seeds";
    const defH = "#222426";
    const defA = "#5ea1ff";
    const defLH = "#e6e9ee";
    const defLA = "#2b7fff";
    let mode = localStorage.getItem(mk);
    if (mode !== "light" && mode !== "dark" && mode !== "system") mode = "dark";
    const dark =
      mode === "dark" ||
      (mode === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
    let seeds = null;
    try {
      const raw = localStorage.getItem(sk);
      if (raw) {
        if (raw.charAt(0) === "{") {
          seeds = JSON.parse(raw);
        } else {
          const p = raw.split(/[,\s]+/).filter(Boolean);
          if (p.length === 4)
            seeds = {
              lightHeader: p[0],
              lightAccent: p[1],
              darkHeader: p[2],
              darkAccent: p[3],
            };
        }
      }
    } catch (_e) {}
    const h = dark ? seeds?.darkHeader || defH : seeds?.lightHeader || defLH;
    const a = dark ? seeds?.darkAccent || defA : seeds?.lightAccent || defLA;
    const r = document.documentElement;
    r.setAttribute("data-theme", dark ? "dark" : "light");
    r.style.setProperty("--header", h);
    r.style.setProperty("--accent", a);
  } catch (_e) {
    const r2 = document.documentElement;
    r2.setAttribute("data-theme", "dark");
    r2.style.setProperty("--header", "#222426");
    r2.style.setProperty("--accent", "#5ea1ff");
  }
})();
