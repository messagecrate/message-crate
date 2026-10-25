# User interface records

This directory holds records of Message Crate's user interfaces, past and
present: what each screen shows, and comparisons between the product UI in
`web/` and the legacy browse UI in `web-next/`. It also keeps the record of
the removed Slint desktop GUI.

The directory holds three records:

- [`web-vs-web-next.md`](web-vs-web-next.md) lists each function web-next
  offers that web does not, as of 2026-10-24, with screenshots of both apps
  in [`2026-10-24/`](2026-10-24/).
- [`web-next.md`](web-next.md) records web-next's screens as of 2026-09-05,
  with its screenshots in [`web-next/`](web-next/). Its feature inventory
  predates the comparison above.
- [`legacy-slint-gui.md`](legacy-slint-gui.md) records the screens of the
  removed Slint desktop GUI as of 2026-08-31. Its images were drawn from the
  `.slint` sources, not captured from the running app, and they sit with
  their HTML sources in [`legacy-slint-gui/`](legacy-slint-gui/).

A new set of screenshots goes in a directory named for the date it was
taken, with one subdirectory per app. A screen both apps have carries the
same file name in each, so the pairs line up.
