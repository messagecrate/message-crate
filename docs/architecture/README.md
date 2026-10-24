# Architecture

Architecture shows the current software shape and the story that makes it
legible. Rules are the constraints the architecture must keep satisfying.

The two live side by side here and are written differently:

| Kind | Architecture (this directory) | Rules (`rules/`) |
|---|---|---|
| Answers | What exists, how it fits, and the few reasons that make the shape make sense | What must hold for any change |
| Tense | Present: what the code does today | Imperative: what a change must keep |
| Checked against | The repository as it is | A proposed diff, by review and by tests |
| Shape | Narrative with diagrams, from the whole system inward | One entry per rule, with a stable id, its reason, and what was rejected |

A person learning the system reads a page here. Anyone changing it, person
or agent, reads the rules for that area first, and a pull request is graded
against them. A rule is written when it is decided, not when the code catches
up; where the code does not follow one yet, an open issue says so. A
disagreement between an architecture page and the code is a bug in one of
them.

Rules carry their own reasons, so a routine decision needs no ADR. An ADR in
`docs/adr/` records one change on one date; a later change is added under a
dated "Amended" heading, and the original text is not rewritten, except
that a path it names follows the file when the file moves. A rule
references the ADR behind it when there is one. `CONTEXT.md` defines the
words; both kinds of document use them and redefine none.

## Pages

| Architecture | Rules | Covers |
|---|---|---|
| [The server's log](server-log.md) | [rules](rules/server-log.md) | What the server records about its own work, how a line travels to the owner's screen, and what never reaches it |
| [How contacts are made and changed](how-contacts-are-made-and-changed.md) | | What an import of a backup does to contacts and identities, what an address book load does, and how the two differ |
| | [Import Run logs](rules/import-run-logs.md) | The desktop app's log of each Import Run and of itself: where they are kept, who reads them, and what a line holds |
| [Media](media.md) | [rules](rules/media.md) | The life of an attachment from the import to the screen: the original, the Thumbnail and the Preview, the pass that makes them, which one a screen loads, and how a video streams |
| | [The HTTP interface](rules/http-api.md) | Every rule the `/v1` routes follow: identifiers, naming, methods, status codes, lists, failures, credentials, runs, the generated reference, and how the server code is layered |
| | [Outbound connections](rules/outbound-connections.md) | The two hosts Message Crate ever connects to, and no telemetry |

Two documents are not yet split into the two kinds. Each holds its
architecture and its rules in one file, with the rules under a `Rules`
heading, until its own pull request separates them:

| Document | Covers |
|---|---|
| [Contacts, identities and messages](contacts-identities-and-messages.md) | The people model: what a contact and an identity are, how conversations and messages attach to them, and what an import creates |
| [The search language](search.md) | The language typed on the Contacts, Conversations and Messages lists: its grammar, values, each list's defaults, and what every word means on every list |

A rules document with no architecture page beside it gets one when someone
needs to explain that area; the rules are complete on their own.
