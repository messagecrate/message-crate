# The HTTP interface

Every rule the server's `/v1` interface follows, in one place. How a request
travels through the server and the shapes every route shares are in [The HTTP
interface](../http-api.md). This document is what a change must not break. The
interface is part of the architecture, so its rules are written down when they
are decided, not when the code catches up: a design is argued against this
file, a pull request that touches a route is graded against it, and a route
that breaks a rule here is a bug. Each rule carries its reason, and where an
alternative was weighed and turned down, one line says so, so the question is
not reopened by accident. Each rule has an id, `API-n`, which code comments,
tests and reviews cite. The numbers run through the sections in document order.
A rule's id never changes. A rule that stops holding is removed, and its id is
not reused.

This file states what every route must do. The generated reference (see
API-120) states what each route does today. Where the two differ, an open issue
names the routes still to change; this file never lists that work itself. A
change to a rule is made here, in the same pull request as the code when the
code changes with it. Why the rules live here and not in `docs/adr/`:
`docs/adr/0011-the-http-interface-has-one-rules-document.md`.

## Identifiers

### API-1 Every resource with a row is addressed by its integer id

Every resource with a row is addressed by its integer id in the URL:
`/v1/contacts/{id}`, `/v1/accounts/{id}`, `/v1/accounts/{id}/api-tokens/{id}`.
Names are for people: they appear in the web app's routes, in the search
language, and in the text of a Saved Search, and the web app turns a name into
an id inside the module that owns the collection, never in a screen.
Why: a name is mutable, needs URL-encoding, and must be matched
case-insensitively on every request, and a rename would re-key the resource.

Rejected: the name in the path for Contact Groups and Message Tags. It keeps
every reference to a group the same kind of thing, and it makes the rule
"id, except where the name is unique". One rule is worth more than the
symmetry.

Rejected: opaque string ids for accounts and API tokens. They were UUIDs from
the first draft with no reason recorded. An integer in a URL tells a stranger
nothing they can act on, because every route checks the caller against the
row.

### API-2 An Asset is addressed by the SHA-256 of its contents

The one exception to API-1 is an Asset, addressed by the SHA-256 of its
contents: `/v1/assets/{sha256}`. Why: the file exists before the database
does, the client must know the hash before an upload can be deduplicated, and
two uploads of one file must be one asset.

### API-3 An Asset's Preview and Thumbnail are addressed under the original's hash

An asset's Preview and Thumbnail have no address of their own. They are
`/v1/assets/{sha256}/preview` and `/v1/assets/{sha256}/thumbnail`, under the
hash of the original, and the attachment says whether each exists in
`preview_mime_type` and `thumbnail_mime_type`.
Why: the client holds the original's hash and has no use for theirs, and
another hash on the attachment would be another thing to address by.

### API-4 The Message Crate's own id is not an address

The Message Crate's own id, `id` on `GET /v1/server`, is not an address. It
is an opaque string of 32 hexadecimal digits that the server writes when its
database is made, and no route takes it in a URL. Why: it exists to tell two
Message Crates apart, at one address or across a rebuilt database, whose
integer ids start at the same numbers, so it must be random rather than
counted, and a client only ever compares it.

## Words on the wire

### API-5 The interface uses the words `CONTEXT.md` defines

Paths, fields, parameters, summaries and problem details use the words
`CONTEXT.md` defines, and never the words it lists under _Avoid_: an
Identity, never a handle (`/v1/contacts/unmatched-identities`); a
Conversation, never a thread; an Import Run, never an import session. "Handle"
stays the name of a table and nothing a client sees.
Why: the web app, the docs and the interface name one thing one way, and the
generated types carry the interface's words into the web app's code.

## Fields

### API-6 A success answer sends every field its type has, `null` when it has no value

A success answer sends every field its type has, as `null` when the field has
no value, and never leaves one out. The reference marks every property of a
success answer required, and nullable where it can be empty, so the web app's
generated types read `field: T | null`, never `field?: T | null`.
Why: a client sees the same keys in every answer of a type, a key a client
forgot to handle is a type error in the generated types rather than an
`undefined` at run time, and a reader has two states to handle, a value and
`null`, not a third, absent.

Rejected: leaving out a field that has no value. It makes the answers a little
smaller, but they are small JSON pages, and every reader would have to handle
a key that is sometimes there.

### API-7 A schema a request body also names keeps the request's optional fields

A schema that a request body also names keeps the request's optional fields,
because a request may still leave out a field it has no value for.
`ExportScope` is the one: it is the body of `POST /v1/exports` and is answered
back on the run, and the server sends both of its lists.

### API-8 A failure is not a success answer

A failure is not a success answer. A problem document carries the members
API-52 gives it and leaves out the rest, because RFC 7807 defines `detail` as a
string, and an extension member such as `retry_after` belongs to the problem
types that carry it.

## Naming a route

### API-9 A collection is plural, and a singular path names a singleton

A collection is plural, and a member is `/{collection}/{id}`. A singular path
is legal only for a singleton: one per installation (`/v1/server`), or one per logged-in
credential (`/v1/session`). `/v1/trash` is a singleton by the same rule.

Rejected: `/v1/auth/login` and its siblings. `/v1/auth` was neither a
collection nor a singleton, so the verb rule could not reach it. A Session is
one per logged-in credential, which makes it a singleton, and login, logout and
check are its `POST`, `DELETE` and `GET`.

### API-10 The Demo Account is a singleton of the server

The Demo Account is managed as a singleton of the server,
`/v1/server/demo-account`: `GET` says whether it exists, is being built, or
failed to build, and `PUT {size}` adds or resets it. It is deleted as any
account is, `DELETE /v1/accounts/{id}`. Why: adding it cannot be a verb on
`/v1/accounts/{id}`, because there is no member to address until it exists,
and there is at most one per Message Crate.

### API-11 A path segment names a resource, never a caller's role

A path segment names a resource, never a caller's role. Who may call a route is
decided in its handler, so a change to permissions never renames a URL.
`/v1/accounts` is one collection for the administrator and for the account
itself; there is no `/v1/admin/` and no `/v1/account`.

Rejected: a role prefix such as `/v1/admin/`. It reads well until permissions
change and a URL has to be renamed to match.

### API-12 A verb is a sub-resource of a member, and only for an action that is not a field write

A verb is legal only as a sub-resource of a member, and only when the action is
not a field write: it crosses a state machine, touches rows other than the
addressed one, or has no field to write. `setup`, `complete`, `cancel`,
`discard`, `trash` and `restore` pass. Setting a run's progress marker does
not; that is `PATCH /v1/imports/{id} {stage}`. A verb that merely spells an
HTTP method never passes: deleting an account is `DELETE /v1/accounts/{id}`.

Rejected: no verbs at all. Modeling every action as a field write turns
trashing into `PATCH {"trashed": true}`, and trash is the only door to
permanent deletion, which `POST .../trash` says and a field write does not.

### API-13 Membership lives under the collection that owns it

Membership lives under the collection that owns it:
`GET` and `PATCH /v1/contact-groups/{id}/members`, with `{add, remove}` id
lists.

### API-14 A read whose selector is too large for a query string is a `POST` named for what it returns

A read whose selector is too large for a query string is a `POST` named for
what it returns, never for the verb that computes it:
`POST /v1/contacts/summaries`, `POST /v1/contacts/unmatched-identities`,
`POST /v1/contacts/address-book`.

### API-15 A choice between two lists is a path segment, never a parameter

A choice between two different lists is a path segment, never a parameter:
`/v1/search-fields/contacts`, `/v1/search-fields/conversations` and
`/v1/search-fields/messages` are three fixed lists, not one list read with
`?list=`. Why: a parameter narrows a list;
choosing which list to read is choosing a resource, and the path does that.

### API-16 A route nests at most two levels deep, with one exception at three

Two levels of nesting. The multipart upload,
`/v1/assets/{sha256}/uploads/{upload_id}/parts/{part}`, is the one exception at
three.

### API-17 `/health` is the only route outside `/v1`

`/health` is the only route outside `/v1`.

### API-18 Path segments are kebab-case, and fields and query parameters snake_case

Path segments are kebab-case; every field and query parameter on the wire is
snake_case.

## Methods

### API-19 Each method means one thing, and a read is a `GET`

`PATCH` updates part of a resource. `PUT` replaces one. `POST` creates, or
runs an action named by a verb sub-resource. `DELETE` removes. A read is a
`GET`, except the large-selector `POST` of API-14.

### API-20 A read by id takes no filter

A read by id takes no filter. `GET /v1/conversations/{id}/messages` opens a
conversation; searching inside one is `GET /v1/messages?q=in:#{id} …`.
Why: opening and searching answer different questions. The read by id answers
`404 Not Found` for an id the caller does not hold and shows a conversation in
the trash; a search answers an empty page and leaves the trash out. A filter on
the read by id is a second search that can drift from the first, as `?year=`
beside `date:` did.

### API-21 A file is the request body, and a file the server writes is answered by a `POST` named for it

A file the server reads is the request body, with `Content-Type` naming its
format, and anything about how to apply it is a declared query parameter.
`POST /v1/contacts` takes the address book as a `text/csv` body and
`mode=append|edit`; any other `Content-Type` is `415 Unsupported Media Type`.
The file the server writes is answered by a `POST` named for it,
`POST /v1/contacts/address-book`, whose body `{q, ids}` selects the contacts
and whose answer is `text/csv` with a `Content-Disposition` filename.
A route that reads one file and writes another does both:
`POST /v1/contacts/vcard-address-book` takes a vCard file as a `text/vcard`
body and `country=` as a declared query parameter, and answers the address
book written from it, named for what it returns.
Why: a body that is the file cannot carry a mode field, and a JSON envelope
around a file base64-encodes it for nothing. Rejected: `?format=csv` on
`GET /v1/contacts`, which cannot carry the checked ids and is the `fields=`
idea in another coat; `Accept: text/csv` on the list, which makes one list
negotiate where every other answers JSON.

### API-22 Behavior that differs by caller lives inside one handler

Behavior that differs by caller lives inside one handler, not in two routes.
`PUT /v1/accounts/{id}/password` is one route: the administrator sets another
account's password without the current one, a user account changes its own on
its session alone, and the administrator changing its own must supply the
current one, because that account reaches every other.

## Status codes

### API-23 A creation answers `201 Created` with a `Location` naming the new resource

A creation answers `201 Created` with a `Location` header naming the new
resource, whatever the method that made it: a `PUT` that stores an asset the
database did not hold, and a setup that makes the administrator's Session
(`Location: /v1/session`), both answer `201 Created`. A create that takes a
batch answers `200 OK` with a summary of what was created, updated and
skipped, because no single resource was made. A Media Link is made though
nothing is stored: `POST /v1/assets/{sha256}/media-links` answers
`201 Created` with the link's own URL in `Location`, which the Session that
made it can `GET`.

### API-24 A write with nothing to return answers `204 No Content`

A write with nothing to return answers `204 No Content`.

### API-25 A write the server finishes after it answers is `202 Accepted`

A write the server finishes after it answers is `202 Accepted`, with the
resource in its body and a `status` that says the work is under way. The
client reads the resource with `GET` until `status` changes; a second write
while the first is under way answers `409 Conflict`.
`PUT /v1/server/demo-account` is the one such route: it removes the Demo
Account and builds it again, which takes from seconds to a minute, and
answers with `status` `building`.
Why: an answer held for a minute is lost to a closed tab or a proxy's
timeout while the work carries on unseen, and `200 OK` would say the Demo
Account is there when it is not yet.

### API-26 A name collision, a wrong state, or the wrong resource answers `409 Conflict`

A name collision answers `409 Conflict`. So does an action on a resource in
the wrong state: deleting before trashing, a setup on a Message Crate that is ready, a batch
or a `complete` on a finished run. So does an action asked of the wrong
resource, `state-conflict` with `detail` naming the right one: a country
picked on a contact for a number whose `+` form is one of the account's own
identities answers it from `PATCH /v1/contacts/{id}`. Why: the merge makes
the number the account's own, so it belongs to `PATCH /v1/accounts/{id}`,
which runs the with-yourself rule and takes the number off its contact.
`state-conflict` fits because the contact's number is in a state, held by
the account, that this route does not change; `422 Unprocessable Entity`
would say the request broke a rule the client could fix by rewriting it, and
no body sent to this route can do the merge.

### API-27 A request that would join two records answers `409 Conflict` until the client confirms with `merge: true`

A request that would join two records into one, and so cannot be undone,
answers `409 Conflict` with nothing changed the first time, and the client
confirms by sending the same request again with `merge: true`. The problem
names the other record, in `detail` and in an extension such as
`identity-exists`'s `holder`, so the client can ask the person. A country
picked for a phone number whose `+` form another identity holds is the one
such request (`set_identity_country` in `PATCH /v1/contacts/{id}` and
`PATCH /v1/accounts/{id}`). Why: the server is the only party that knows the
two are one record, and only once it has worked out the `+` form, so a
client cannot ask before it sends. Rejected: merging without asking,
because a merge moves conversations and messages and cannot be undone; and
a separate preview route, which would work the same form out twice while
another request between the two could change the answer.

### API-28 A failed credential is `401 Unauthorized`, a refused one `403 Forbidden`, and every refused login `401 Unauthorized`

A failed credential answers `401 Unauthorized`. A refused one, including a
token without the needed scope and a disabled account, answers
`403 Forbidden`. A login is the exception: every refused login, a
disabled account's included, answers the same `401 Unauthorized`
(`invalid-credentials`).
Why: a disabled account's own answer would come only after the right
password, so a guesser would learn which guess was right, and the password
works again once the administrator enables the account. The Audit Trail records the
real reason.

### API-29 Disabling an account ends its Session

Disabling an account ends its Session, so the Session token it held
answers `401 Unauthorized` from then on, after the account is enabled
again too; only its API tokens answer `403 Forbidden` (`account-disabled`)
while it stays disabled. A login whose password check the disable
overtook opens no Session.
Why: an administrator who learns that someone else holds the Session token can
shut it out and give the account back to its holder, and enabling the
account brings back neither the token nor the Media Links signed under it
(#2074).

### API-30 An absent or unaccepted `Content-Type` is `415 Unsupported Media Type`

A `Content-Type` that is absent or unaccepted answers
`415 Unsupported Media Type`, on every route that takes a body. On a route
whose body is optional, a request with no body is read as no body, and a
body sent without a `Content-Type` is still `415 Unsupported Media Type`.

### API-31 A request that cannot be read is `400 Bad Request`, and nothing else is

A request that cannot be read is `400 Bad Request` (`malformed-body`): JSON
that does not parse, a body of the wrong type, or a body whose connection
broke before it arrived. Nothing else is `400 Bad Request`. A broken
connection is this type and not one of its own because the server has no
request to judge: the client that sent it has usually gone, and one that
is still there sends the body again, as it would after any broken
connection.

### API-32 A body the server stopped reading because it took too long is `408 Request Timeout`

A body that the server stopped reading because it took too long is
`408 Request Timeout` (`body-stalled`): a body that went `[server]
body_stall_secs` (60) with no byte arriving, or was still arriving
`body_total_secs` (300) after the read started. Every body is held to
the limits (API-65). Why: the connection failed, not the data, and
`malformed-body` told the person their data was bad. The client is still
there to read the answer, so the answer says that running the import
again continues it. The desktop app shows it and does not send the piece
again by itself, as it does not for any `4xx`
([#2897](https://github.com/messagecrate/message-crate/issues/2897)).

### API-33 A request that was read and broke a rule is `422 Unprocessable Entity`

A request that was read and broke a rule is `422 Unprocessable Entity`,
whether the rule was on a query parameter, a path segment or a body field: a
field or parameter missing or blank, a value out of range, an id that names
no row the caller holds in a list of ids to add, a body that does not match
the hash it is addressed by, and a search query that uses a word its list
does not have. Axum's own rejections follow the same line. A missing
parameter is one entry in `validation-failed`'s `errors`, not a problem type
of its own. A search that does not parse keeps its own type,
`search-query-invalid`, because the client's remedy differs (rewrite the
query), and answers `422 Unprocessable Entity` like every other.

Rejected: a search query that does not parse as `400 Bad Request`, "the query
could not be read". The request was read; the query is a value that broke the
rules of the search language, which is what `422 Unprocessable Entity` means.
One line with no exception is easier to hold than a line with one.

### API-34 A request that was read, broke no rule, and completed answers a `2xx`, even when it changed nothing

A request that was read, broke no rule, and completed answers a `2xx` even
when it changed nothing. The status code says whether the action ran or
failed; what the action found or changed is the body's job. An id in the
`remove` list of a `{add, remove}` membership patch that names no member is
ignored: the set is already in the state the caller asked for, so the route
answers `200 OK` with `{added, removed}`, `removed` counts only the rows
deleted, and a retried request succeeds. An id in `add` that names no row
the caller holds stays `422 Unprocessable Entity`, because adding a row the
caller does not hold breaks a rule rather than completing with an empty
result.

### API-35 A database too busy to take the request answers `503 Service Unavailable` with `Retry-After`

A request the database was too busy to take in time answers
`503 Service Unavailable` (`database-busy`) with `Retry-After`: SQLite
answered `SQLITE_BUSY` once the busy timeout ran out, or no pool connection
came free within the pool's acquire timeout (sqlx's `PoolTimedOut`). Why:
neither is a bug in the server, and the same request succeeds once the
import batch or the requests holding the database finish, so the remedy is
to wait and send it again, not to report a fault
([#2084](https://github.com/messagecrate/message-crate/issues/2084)).
`/health` is the exception (API-57).

### API-36 Every `401 Unauthorized` carries a Bearer challenge

Every `401 Unauthorized` carries `WWW-Authenticate: Bearer
realm="message-crate"`, whichever problem type it answers:
`authentication-required`, `invalid-credentials` or `media-link-invalid`.
When the request sent a Bearer token and the server refused it (unknown,
expired, or its account gone), the challenge adds `error="invalid_token"`.
A request that sent no token, or an `Authorization` header that is not
`Bearer <token>`, gets no error code. Why: RFC 9110, section 15.5.2,
requires the header on every `401`, and RFC 6750, section 3.1, gives the
error code and says a request that carried no token gets none. The
OpenAPI document declares Bearer schemes, so generic HTTP tooling reads
the challenge. The web app and the desktop app read the problem `type`
instead. A refused Media Link gets the plain challenge, because a Media
Link is not a Bearer token and the challenge names the way to send one.

### API-37 The asset routes answer one byte range with `206 Partial Content`

The three routes that answer an asset's bytes, `GET /v1/assets/{sha256}`,
`GET /v1/assets/{sha256}/preview` and `GET /v1/assets/{sha256}/thumbnail`,
answer a `Range` of one byte range
(`bytes=0-499`, `bytes=500-`, `bytes=-500`) with `206 Partial Content`,
those bytes, and `Content-Range: bytes <first>-<last>/<length>`. A range
that selects no byte of the file answers `416 Range Not Satisfiable`
(`range-not-satisfiable`) with `Content-Range: bytes */<length>`. Any other
`Range` answers `200 OK` with the whole file, as RFC 9110 allows: another
unit, several ranges, a range that cannot be read, or an `If-Range` that
does not name this file. Every answer carries `Accept-Ranges: bytes`. The
original's `ETag` is its fingerprint, which `If-Range` may name; a Preview
and a Thumbnail have none, so a `Range` sent with `If-Range` gets the whole
file.
Why: a video plays from a media element that asks for the file a range at
a time and seeks by asking for another (`docs/architecture/rules/media.md`).
Rejected: answering several ranges as `multipart/byteranges`. No media
element sends several, and the whole file is a correct answer to them.

### API-38 An unknown `/v1` path (`404 Not Found`) and a wrong method (`405 Method Not Allowed`) answer a problem document

An unknown `/v1` path answers `404 Not Found` as a problem document, and a
wrong method `405 Method Not Allowed`, never Axum's plain text.

### API-39 Neither a success nor a request carries an `ok` flag

There is no `ok` flag on any success, and none in any request: the status
carries the meaning, and a run's outcome is its `status`.

## Lists

### API-40 Every list answers a page, `{items, total, limit, offset}`

Every list route answers a page, `{items, total, limit, offset}`, and takes
`offset` and `limit`. A list the person curates by hand (groups, tags, saved
searches, API tokens), a fixed reference list (`/v1/search-fields/contacts`),
and a `POST` that reads all answer a page. The one exception is the server's
log lines, which are written while they are read and page from a line
(API-51). The list key is always `items`.
Why: the web app has one paged type and one hook, and a second shape is a
second convention.

Rejected: cursor paging. Stable under concurrent inserts, but nothing inserts
rows under a running read on a self-hosted server, and every screen that shows
"51–100 of 4,213" needs `total`.

Rejected: a bare `{items}` for small lists. One justified exception is still
two conventions, and a group's member list has no bound the server enforces.

### API-41 The Messages list's page carries `search`, the one key beside the four

The Messages list, `GET /v1/messages`, is the one page with a key beside the
four: `search`, which says how the server read the request. `search.sort` is
the order the page is in, spelled as the `sort` parameter takes it, and
`search.terms` holds the free-text terms the query ranks by, each a `text` and
whether it is a `prefix`. The key describes the query, not the rows, so every
page of one query carries the same value. Why: with no `sort`, the server
picks the order from the query, best match first when it has a free-text term
and newest first when it has none (`docs/architecture/rules/search.md`,
SEARCH-8), and the web app shows that order in its sort menu and draws the
terms in bold. Only the module that parses the search language knows which
words those are
(`docs/adr/0004-one-search-language-compiled-in-one-module.md`). The web app
once read them from `q` with a parser of its own, a second copy of the grammar
that nothing held to the first (#1538). Rejected: a route of its own that
reads a query and answers its terms. A query is not a resource, and every new
search would wait for that answer before it could ask for its first page in
the right order.

`openapi/document_rules.rs` names the route and its schema,
`ListMessagesResponse`, and fails any other page with a key beyond the four.
It knows a page by its four keys, not by its name, so a page under a new
name is held to the same rule.

### API-42 A `POST` that reads the rows its body names answers the whole body as one page

A `POST` that reads the rows its body names — contact summaries, unmatched
identities — answers the whole of that body as one page and takes no `offset`
or `limit`: `total` is the row count, `limit` is the cap the body is held to,
`offset` is 0. Why: the body already says which rows to read and how many it
may name, so a second bound would only let a caller ask for rows it did not
name, or hide rows it did.

### API-43 A count of the whole set belongs on the resource the set hangs off

A count that describes the whole set rather than the page belongs on the
resource the set hangs off, not beside `items`. An Import Run's tally of
contacts created and changed is on the run's own record, and the contacts are
a page. Why: a page can only count its own rows, and a field beside `items`
that counts something else is a second shape.

### API-44 A list's row carries nothing that grows without bound

A list's row carries nothing that grows without bound. Where a resource holds
such a collection, as an Import Run holds its issues and its notes, the list
answers how many in a count (`issue_count`, `note_count`) and the resource's
own `GET` answers the collection. `GET /v1/imports` and an account's own
`GET /v1/accounts/{id}/imports` answer each run as an `ImportRunSummary`, with
the counts and without the issues or notes. The administrator's
`GET /v1/accounts/{id}/imports` answers each as an `AdminImportRunSummary`,
which also counts them and never carries them. `GET /v1/imports/{id}` answers
the issues and the notes. Each count is read in the list's own statement, never
by a statement per row. Why: `limit` bounds a page's rows and nothing else, so
a collection inside each row left a page's size to whatever the runs recorded.
Forty WhatsApp runs of 20,000 skipped files each made one page of Settings →
Storage carry about 800,000 issues, and one statement per row made a page of
500 runs a thousand statements (#1559).
Rejected: capping how many issues a run stores. It bounds the list by
throwing away the diagnostics the run exists to keep.

### API-45 An Import Run's attachments without a Thumbnail or Preview are only on the administrator's read of the run

The attachments an Import Run wrote whose Thumbnail or Preview could not be
made are such a collection too (API-44). The administrator's
`GET /v1/accounts/{id}/imports/{import_id}` answers an `AdminImportRun`,
which adds them (`not_made_attachments`) and their count by kind of failure
(`not_made_counts`). The administrator's list carries neither, not even a count.
Why: a count would read every attachment of every run on the page, where
the issue count reads one small table, and the administrator reads the failures on
the run's record (#2636).

### API-46 `limit` is 1 to 500, `offset` at most 50 000 on a browse list, and a value outside is refused

`limit` is at least 1 and at most 500, default 40, on every list including an
Export Run's messages. `offset` is at most 50 000 on the browse lists. A value
outside the range is `validation-failed`, never a silent clamp. One
conversation's messages, `GET /v1/conversations/{id}/messages`, is not a browse
list and has no `offset` cap. Why: every message of a long conversation must
be reachable, and a cap would leave the rest of a long thread out of reach.

### API-47 One conversation's messages can be read beside one message

One conversation's messages can also be read beside one message, in place of
`offset`: `around={message_id}` answers the page with that message in the
middle, and `before={message_id}` and `after={message_id}` the page just
before or just after it in the page's order, without it. The answer is the
same page, and its `offset` says where the page sits, so `total` and the
position stay known. A request sends at most one of `offset`, `around`,
`before` and `after`, and a message the conversation does not show (another
conversation's, a duplicate, or none) is `validation-failed`. Why: a jump to a
message (a search result, a Find match, the first message of a year) does not
know the message's offset, and a screen that scrolls from there reads the next
page from the message at its edge rather than from a number that an import
or a deletion in between would shift. This is not the cursor paging rejected
under API-40: the page keeps `total` and `offset`, and the parameters name a
message, not an opaque token.

### API-48 Sorting is `sort=-field,field`

Sorting is `sort=-field,field`: comma-separated keys, a leading `-` for
descending. Each list declares the keys it accepts, and an unlisted key is
`validation-failed` naming the accepted set. There is no separate `order=`.

### API-49 Filtering is the search language in `q`, and nothing else

Filtering is the search language in `q`, and nothing else. The one exception
is a list with no search language, which may take a filter parameter whose
values are the ones its rows store. The Import Run and Export Run lists, the
Audit Trail and the server's log lines are the only such lists:
`GET /v1/imports?status=running`, `GET /v1/exports?status=completed` and their
twins under an account, `GET /v1/audit-trail?deleted_account_id=7`, which reads
one deleted account's entries and runs by the id they keep, and
`GET /v1/server/log-lines?level=warn&text=import`, where `level` keeps that
level and the more severe ones and `text` keeps the lines whose text, after
the time and the level, holds it, ignoring case. A log line is text, not a
row with fields a search language could name, so `text` is the whole of its
search. There is no `fields=` selection.

Rejected: `fields=`. It turns one resource into many shapes, and the web app's
generated types could only express that with every field optional.

Rejected: query-parameter filters beside the search language. `?date_gte=2019`
next to `q=date:>2019` is two ways to ask one question.

### API-50 A query parameter a route does not declare is refused

A query parameter a route does not declare is `validation-failed`, naming the
parameters the route accepts. The `media_link` of a Media Link is declared by
its security scheme, an API key in the query, and counts as declared on the
three routes that take it. Why: a typo (`limt=10`) or a guess at a
convention this file rejects (`order=`, `fields=`, `year=`) would otherwise be
answered as though it had been obeyed.

Rejected: ignoring a query parameter the route does not know, the forgiving
default of most web servers. The server's clients are its own apps and programs
written against the reference, and a silent wrong answer costs them more than
a refusal.

### API-51 The server's log lines page from a line

The server's log lines, `GET /v1/server/log-lines`, are the one list that pages
from a line instead: `{items, limit, has_more}`, newest first, and `after={id}`
reads the lines older than the line with that id. It takes no `offset` and
answers no `total`. Why: the server writes lines while the administrator reads them,
its answers to the reading included, so the reason cursor paging is rejected
under API-40 does not hold for it, and a `total` would mean counting up to 250
MB of files for every page. The id names a line, as `around`, `before` and
`after` name a message, and is not an opaque token
(`docs/architecture/rules/server-log.md`).

## Failures

### API-52 Every failure answers an RFC 7807 problem document

Every failure answers an RFC 7807 problem document as
`application/problem+json`, carrying `type`, `title`, `status`, `detail` and
`request_id`. A validation failure replaces `detail` with `errors`, a list of
every rule that broke rather than the first.

Rejected: a flat `{error}` body. It gives a client nothing to branch on except
the sentence.

### API-53 `detail` is one sentence in lowercase with no final period, and the client decides how it starts and ends

`detail`, and each entry of `errors`, is one sentence in lowercase with no
final period, naming the thing and what is wrong with it: `asset not found`,
`new passwords do not match`. A word always written with a capital keeps it,
such as a header (`Content-Type must be text/csv`) or an abbreviation
(`API token not found`). The client decides how the sentence starts and ends
on its screen, because it may show the sentence alone or joined with others.
The web app joins a validation failure's `errors` with `; ` into one line.
Every screen that shows the sentence as a sentence of its own, on a line of
its own or after another sentence that ends with a period, shows it through
one function, `errorLine` (`web/src/lib/apiErrorMessage.ts`), which
capitalizes its first letter. A sentence that starts with a name a capital
would change keeps its first letter: a `search-query-invalid` refusal about
one word starts with that word, as
`from: is not a Contacts word; it works on Messages`, and a server refusal
that starts with a field's snake_case name or indexed path, as
`asset_max_bytes must be at most …` or `identities[0].address: …`, names the
field as the API spells it. A
sentence joined after a prefix, as `Could not load contacts: …`, is shown as
the server wrote it.
For a `5xx`, the web app ends the sentence with the problem's `request_id`, as
`internal server error (request 4f3c…)` (`problemFromBody` in
`web/src/lib/api.ts`). Why: a `5xx` names no cause to the client, so the id is
what a person can quote to find the failure's line in the server's log
([The server's log, LOG-13](server-log.md#log-13-every-line-carries-its-requests-id-spawned-work-included)).

### API-54 `type` is the URL of the problem type's page, and the code is the registry

`type` is the URL of a page under
`messagecrate.app/docs/developer/reference/errors/`, one page per problem type.
The code is the registry: each type is declared once in
`crates/server/server/src/problem.rs`, the pages are generated from it, and a
test fails when the checked-in pages drift. Only `500 Internal Server Error`
uses `about:blank`, because a page about it could say nothing a reader could
act on. The taxonomy is per problem, not per status: the test for a new type
is that a client's remedy differs. A disabled account is its own type
(`account-disabled`), because the remedy (ask the administrator to enable
the account) is not the remedy for a caller who is not the administrator
(`not-the-admin`).

### API-55 Every response carries an `x-request-id` the server makes

Every response, success or failure, carries an `x-request-id` header, a UUID v4
the server makes; a request id a client sends is ignored. Problem bodies repeat
it as `request_id`. The id joins the request's tracing span so every log line
under a request carries it, work the request spawns included
([The server's log](server-log.md)). RFC 7807's `instance` stays unused,
because it is defined as a URI reference and a bare id is not one.

Rejected: a request id on `5xx` only. A body whose members vary by status is
the drift the one-shape rule (API-52) exists to stop.

### API-56 A handler that panics answers a `500 Internal Server Error` problem document

A handler that panics answers the same `500 Internal Server Error` problem
document, with its `request_id`, as any other internal failure: the
innermost layer of the router catches the panic (`answer_panic` in
`crates/server/server/src/middleware.rs`), so every layer outside it, CORS and
the request's log line included, treats the answer as it treats any `500`.
Why: an uncaught panic closed the connection with no answer, so the person
saw a network error with no id to report
([#2060](https://github.com/messagecrate/message-crate/issues/2060)).

### API-57 `/health` answers `ok` only when the database answers `SELECT 1` within 1.5 seconds

`/health` answers `ok` only when the server can take a database connection
and run `SELECT 1` on it within 1.5 seconds (`HEALTH_DATABASE_LIMIT` in
`crates/server/server/src/server.rs`). When it cannot, it fails as a `/v1`
call fails, with a `500 Internal Server Error` problem document. Why: the web
app's login card and the release image's Docker health check ask `/health`
whether a server can answer, and a server whose pool is exhausted or whose
database cannot be opened answers no `/v1` call, so `ok` from the process
alone told them it was up while every page failed. The desktop app's watch
on its own server asks `GET /v1/server`, which reads the database, so the two
disagreed ([#2202](https://github.com/messagecrate/message-crate/issues/2202)).

Why `SELECT 1` and a short limit: `SELECT 1` reads no table, so it never waits
on an import's write lock, and the limit stays under the time
`message-crate-server healthcheck` gives each address it asks (4 seconds, or 2
each when it asks both 127.0.0.1 and ::1), so a failure arrives as the
server's answer. HTTP imports hold at most `MAX_CONCURRENT_IMPORTS`
(`imports_api/run_control.rs`) of the pool's `max_connections`
(`db/engine.rs`), but requests that write wait on SQLite's busy timeout while
an import holds the write lock, and enough of them can take every connection
for that long. The check fails then, though a `/v1` read would wait for a
connection and get one. One failure does not mark the container unhealthy,
because Docker needs three in a row 30 seconds apart. When `/health` answers
with an error, the login card asks once more three seconds later and shows
Disconnected only when both fail, as the desktop app's watch on its own server
needs two failed checks in a row (`checkServerHealthWithRetry` in
`web/src/lib/serverHealth.ts`,
[#2718](https://github.com/messagecrate/message-crate/issues/2718)). A pool
taken for longer than that, up to the 15-second busy timeout, still fails both
probes, and the card shows Disconnected until its next probe finds the server.
That is accepted: while every connection is taken, the card's own login would
wait as well.

Rejected: `503 Service Unavailable` for `/health`. A server that cannot reach
its own database is the `500 Internal Server Error` no page could help with.
That holds when every connection is taken too, though a `/v1` call answers
that with `database-busy`: `/health` is outside `/v1`, and its callers read
only whether it answered `ok`.

### API-58 Every registered problem type is a `4xx` but `server-stopping` and `database-busy`

Every registered problem type is a `4xx` but two, `server-stopping` and
`database-busy`, each a `503 Service Unavailable`. Each is its own type
because the remedy, sending the same request again, is not the remedy for a
`500 Internal Server Error`, and the request was not the client's mistake, so
it is not a `4xx`.

### API-59 A busy database answers `database-busy` wherever in the request it refused

A request that reaches a busy database answers `database-busy`, with a
`Retry-After` of 5 seconds, wherever in the request the database refused it:
the error's chain is searched for sqlx's `PoolTimedOut` or a SQLite
`SQLITE_BUSY` (`is_database_busy` in `crates/server/server/src/api_error.rs`),
so no handler has to map it. The refusal leaves one `WARN` line naming the
type and carrying the chain, and no `ERROR` line
([The server's log, LOG-9](server-log.md#log-9-a-refused-request-names-its-problem-type-never-its-detail)).
The desktop app's HTTP client (`classify_retry` in `crates/libs/http`)
retries a `5xx`, so it retries this with no change of its own. The web app
tries a read that failed with a `5xx` once more (`retry` in
`web/src/lib/routeQuery.ts`). For `database-busy` it first waits the
`retry_after` seconds the server named (`retryDelay` there). The database
was busy because another request, such as an import batch, held the write
lock, or because every database connection stayed in use, and either way a
second try at once meets the same busy database. It never tries a write
again: it shows the write's failure with its `detail`.

### API-60 The database pool holds 8 connections, a request waits 10 seconds for one, and a write 15 seconds for the write lock

The pool (`crates/server/server/src/db/engine.rs`) holds 8 connections: two
import batches and the sweep of unreferenced files after an Import Run
(`asset_store::sweep_after_run`) can each hold one for a whole run. The
background move of a Session's expiry (`auth::ExpiryMoves`, under API-72) holds one more for up to the busy timeout
while an import holds the write lock. Under write-ahead logging the rest serve
reads side by side. A request waits 10 seconds for a connection, and a write
waits SQLite's 15-second busy timeout for the write lock. Why: with 4
connections, two imports and a sweep left one for every other request, and a
request waited sqlx's default 30 seconds for it before answering
`500 Internal Server Error`
([#2124](https://github.com/messagecrate/message-crate/issues/2124)). Each
connection's page cache is capped at about 100 MB, half what it was, so the 8
hold no more memory in all than the 4 did. Not more than 8, because more
connections would raise that total, or shrink each cache further.

### API-61 A server told to stop refuses an import batch that has not started

A server told to stop closes its import slots
(`AppState::on_stop_signal` in `crates/server/server/src/server.rs`), so an
import batch still waiting for a slot is refused with `server-stopping`,
having read none of its body, and stores nothing. A batch that holds a slot
but is still reading its body stops reading at once and is refused the same
way, so the stop never waits for the rest of a body that a client sends
slowly or has stopped sending
([#2878](https://github.com/messagecrate/message-crate/issues/2878)).
The Upload retries it as it retries any `5xx`, waiting at least 15 seconds
before its last try (`LAST_RETRY_WAIT` in `crates/libs/http`). If the server
is not back by then, the Upload pauses without recording the batch's
conversations as failed, and resuming the run sends the batch
([#2727](https://github.com/messagecrate/message-crate/issues/2727)). A
server that cannot be reached, or answers `502 Bad Gateway`,
`503 Service Unavailable` or `504 Gateway Timeout`, pauses it the same way
([#2092](https://github.com/messagecrate/message-crate/issues/2092)). A batch whose
import has started is not refused. Why: a waiting batch used to take a slot
once an earlier batch ended and import in full, so the stop waited for a
whole batch more
([#2203](https://github.com/messagecrate/message-crate/issues/2203)).

### API-62 A stop waits up to 30 seconds for the requests in flight

After a server told to stop closes its import slots (API-61), it waits for the
requests in flight, every route alike, and for the stop of a Demo Account build
and of the Thumbnail and Preview pass, for up to 30 seconds (`STOP_DRAIN_LIMIT`
in `crates/server/server/src/startup.rs`).
A request still running then is cut off, and the server exits after one
`WARN` line naming each such request by its id, method and path
(`crate::in_flight` tracks every request until its answer's body is sent).
An import batch cut off this way is rolled back by SQLite and stores
nothing, and the Upload, which gets no answer, pauses and sends it again
when the run resumes. Why: an import batch can run for minutes, and
`docker stop` killed the server after Docker's 10 seconds, so the batch was
lost all the same, with a log that ended mid-request
([#2122](https://github.com/messagecrate/message-crate/issues/2122)). Both
Compose files set `stop_grace_period: 40s`, and the documented `docker run`
commands `--stop-timeout 40`, so Docker never has to kill a server that
keeps to its own limit. `docker_stop_waits_longer_than_the_drain` fails when
either is no longer than the limit. Rejected: a config key for the limit,
because Docker's stop time would have to follow every value it could be
set to.

## Connections

The server holds every connection to four limits, so one slow or stuck
client cannot hold it
([#2100](https://github.com/messagecrate/message-crate/issues/2100),
[#2984](https://github.com/messagecrate/message-crate/issues/2984)). A
connection that sends no request, or sends one too slowly, is closed after
30 seconds. At most 256 connections are open at once, and the next waits
for one to close. A body that stops arriving, or arrives too slowly, is no
longer read once the body time limits pass. A connection whose client reads
none of its answer for the body stall time is closed. API-63 to API-66 give
each in detail.

### API-63 A connection has 30 seconds to send a request's headers

A connection has 30 seconds to send a request's headers, counted from when
it opens or from the previous answer, and is closed when it has not
(`HEADER_READ_LIMIT` in `crates/server/server/src/connections.rs`). So an
idle connection stays open 30 seconds after its last answer. The desktop
app keeps an idle connection for reuse 20 seconds (`POOL_IDLE_LIMIT` in
`crates/libs/http`), under that, so it never sends a request on a
connection the server is closing.

Why: `axum::serve` builds each connection without a timer, and hyper reads
its own 30-second header limit only with one set, so a device that opened a
connection and sent nothing, or sent its headers or a JSON body a byte at a
time, held a task and a file handle for as long as it liked, and any number
of devices could. On a home network one misbehaving device could starve a
server that has one database file and two import slots. The header read
limit and the connection cap are fixed in the code, not config keys,
because no setup needs others: a working client sends its headers at once.

### API-64 At most 256 connections are open at once

At most 256 connections are open at once (`MAX_CONNECTIONS` in
`crates/server/server/src/connections.rs`). The next one is accepted once one
of them closes, and waits with the operating system until then. 256 is far
above what a few browser tabs and the desktop app's Upload open together, and
under the 1024 open files a Linux process gets by default, which the
connections share with the database and the upload files. API-63's "Why" is
the reason for the cap too, and for its being fixed in the code.

Rejected: a cap on requests in flight (`tower::limit::ConcurrencyLimitLayer`)
in place of the cap on connections. A connection that never sends its
headers is no request, so a cap on requests leaves it uncounted. Under
HTTP/1.1 a connection carries one request at a time, so the cap on
connections bounds the requests too. Axum's `Router::layer` would also give
each route a cap of its own.

### API-65 Every request body is held to the body time limits

Every request body is held to the body time limits, `[server]
body_stall_secs` (60) with no byte arriving and `body_total_secs` (300)
for the whole body, and one that meets either answers `408 Request Timeout`
(`body-stalled`). An import batch, an attachment and a part of one hold
their bodies to the limits themselves, from when they start reading. The
layers hold every other body from when the request arrives
(`limit_request_body` in `crates/server/server/src/middleware.rs`). Why the uploads start their own
clock: an import batch waits for an import slot before it reads, and a
clock started on arrival would count that wait against its body.

Why the body limits are the upload ones rather than figures of their own:
every other body is smaller than an import batch, so the limits a batch
arrives inside hold it too, and one pair of figures is one to explain.

### API-66 An answer the client takes no byte of for the body stall time closes its connection

An answer the client takes no byte of for `[server] body_stall_secs`
(60) closes its connection, and the server lets go of the answer and the
file it reads from (`AnswerStall` in
`crates/server/server/src/connections.rs`). The wait counts only while a
write waits on the client, and starts again with every byte the client
takes, so a download that is still moving, however slowly, is never cut
off, and an answer the server is still making is not timed. The close is
written to the server's log at `INFO`, so the limit firing shows at the
default level and reads apart from a client that hung up, which is
written at `DEBUG`. Nothing limits
how long a whole answer takes to send: an export download or a large
attachment takes as long as the client's link needs.

Why the answer is held to the body stall time rather than a figure of its
own: a client that reads none of an answer is as stuck as one that sends
none of a body, and one figure for "no byte moved" is one to explain. Why
the limit sits on the connection rather than on the answer's body: while
the client reads nothing, the server is waiting to write and does not ask
the body for more, so a timer in the body would never be looked at.

### API-67 The connection limits do not make the server safe to reach from the internet

These limits keep a slow or broken client from holding the server. They do
not make the server safe to reach from the internet: that needs a proxy, as
API-107 says, and what the server
itself must do there is
a separate design
([#2894](https://github.com/messagecrate/message-crate/issues/2894)).

## Content negotiation

### API-68 Outside the routes that answer bytes, `/v1` refuses with `406 Not Acceptable` only an `Accept` header none of whose members matches JSON

`406 Not Acceptable` is answered only when an `Accept` header is present and no
member of it matches `application/json`, `application/problem+json`,
`application/*`, or `*/*`. `application/*` is a media range that matches
`application/json` (RFC 9110), so refusing it would refuse a client that asks
for JSON. A missing `Accept` is a request for JSON. The check runs on every `/v1` route
but the six that answer bytes: `GET /v1/assets/{sha256}`, which streams the
asset's own contents, `GET` and `HEAD /v1/assets/{sha256}/preview` and
`GET` and `HEAD /v1/assets/{sha256}/thumbnail`, which stream its Preview and
its Thumbnail or answer the headers that stream would carry,
`POST /v1/contacts/address-book` and `POST /v1/contacts/vcard-address-book`,
which answer the address book as `text/csv`, and `GET /v1/server/log-files/{id}`, which answers a file of the
server's log as `text/plain`. A media element probes a file with `HEAD` and an
`Accept` such as `image/*`, so refusing that `HEAD` would refuse the probe.
`HEAD /v1/assets/{sha256}` is checked, because it is the probe for whether an
asset is stored and answers JSON. Nothing outside `/v1` is checked.

Rejected: requiring `Accept: application/json`. None of the server's own clients
send one, and the rule would refuse the web app on its first request.

## Caching

### API-69 Every `/v1` answer carries `Cache-Control: no-store` unless its route sets its own

Every `/v1` answer carries `Cache-Control: no-store` unless its route sets a
`Cache-Control` of its own. The rule covers every status and every answer a
layer gives before the handler runs: a `401 Unauthorized` or `403 Forbidden`
from the credential check, a `405 Method Not Allowed`, a `406 Not Acceptable`,
a `413 Payload Too Large`, the `404 Not Found` of a path no route matches, and
the answer to a CORS preflight. The only routes that set their own are the
three asset reads, which send `private`, or `private, no-store` under a Media
Link, once the credential is accepted (see API-75). Their refusals before that point take
`no-store` from this rule. An asset read's handler reaches the account the
read opens only through `AssetReadAccess::answer`, which sets the header on
whatever it answers, so an asset read route added later cannot answer
without it, and a test walks every route the document opens to a Media
Link (#2585). Nothing outside `/v1` is marked: the website's built
files and `/health` keep their own caching.
Why: the answers carry a Session token (`POST /v1/session`), message text,
contacts and attachments. RFC 9111 already keeps a compliant shared cache
from storing an answer to a request that sent `Authorization`, and a browser
does not cache a `POST` by default, but a browser's own cache may still write
a `GET` answer to disk, and a proxy that does not follow the RFC may keep
one. `no-store` says in the answer itself that nothing may keep it.
How: `with_server_layers` in `server.rs` puts one layer,
`no_store_unless_set` in `middleware.rs`, around the whole router, so a route
added later is covered without remembering to. The layer sets the header only
when the answer has none. The OpenAPI document states the rule once, in its
description, as it does for `405 Method Not Allowed`, and the walk over every
operation checks it (see API-125).

Rejected: `private` for every answer. It still lets the browser write message
text and a Session token to its disk cache, and the web app fetches afresh
through its own query cache rather than the browser's.

## Protective headers

### API-70 Every answer carries `Content-Security-Policy`, `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`, each unless the route sets that header itself

Every answer, the website's built files as well as `/v1`, `/health` and the
OpenAPI UI, carries `Content-Security-Policy`, `X-Content-Type-Options:
nosniff` and `Referrer-Policy: no-referrer`, unless the route sets the header
itself (#2073). The policy runs only the server's own script files, never
inline script. It refuses plugins (`object-src 'none'`), because a plugin
runs code the script rule does not govern. It forbids any site to frame a
page (`frame-ancestors 'none'`), so no other site can show the login card
inside its own page and trick a person into typing there. A `<base>` element
may point only at the server (`base-uri 'self'`), so injected markup cannot
send the website's relative script paths elsewhere. No form submits
anywhere (`form-action 'none'`), because the website sends everything with
`fetch` and an injected form could post a password to another site. Fonts
may come only from the server or a `data:` URL (`font-src 'self' data:`), as
in the desktop window's policy; the website loads no font from another host.
Styles may be inline, because the website's
components set `style` attributes. `connect-src`, `img-src` and `media-src`
allow other hosts over `http:` and `https:`, because the login card's
**Change server address** points the website at another server, and allow
`blob:` and `data:`, the attachment and image URLs the website makes itself.
The policy is `CONTENT_SECURITY_POLICY` in `middleware.rs`, and the desktop
window's own policy in `src-tauri/tauri.conf.json` follows the same lines.
Why: the website keeps the Session token in browser storage, and a cookie is
not an option (see API-75), so any
script that runs on the page can read the token and send it anywhere. The
policy keeps such a script from running at all. No injection is known today.
How: three `SetResponseHeaderLayer::if_not_present` layers in
`with_server_layers` wrap the whole router, so a route added later is
covered. The website's first-paint theme script is `web/public/theme.js`, a
file of its own, because the policy refuses an inline one.
Rejected: the policy on the website alone. A `/v1` answer a browser renders,
such as a JSON body opened in a tab, is a page too, and the OpenAPI UI runs
under the same policy without change.

## Credentials and reach

API-71 to API-75 are the credentials, API-76 to API-93 what each reaches,
API-94 to API-104 what a request is held to, and API-105 to API-108 what holds
for every credential.

### API-71 Every route declares which of the three credentials it accepts

Three credentials exist, and the OpenAPI document declares each as a
security scheme with its scopes, so every route says which it accepts.

### API-72 A Session carries the account's own permissions and ends after 7 days unused or 30 days from the login

A **Session** is one per logged-in account or administrator, made by
`POST /v1/session`, or by `POST /v1/server/setup` for the administrator's
first one, and ended by `DELETE /v1/session`. It carries the account's own
permissions: `import`, `export`, `delete`. The administrator's Session
carries none of those and reaches only the accounts collection, the server
settings and the installation's storage totals, because the administrator
holds no messages. It also reads the country list (`GET /v1/phone-countries`),
because the administrator picks the country of a number on an account it
manages. The OpenAPI document marks each route the administrator's Session
reaches with the `admin` scope on the `session` scheme.
A Session ends after 7 days unused, and after 30 days from the login
however much it is used. A login sets its expiry 7 days ahead. A request
that the Session serves moves the expiry to 7 days after the request, but
never past 30 days after the login, and writes it only when the move is
an hour or more, so a Session in use writes at most once an hour. The
move is written in the background, one at a time, so a request never
waits for the write lock an import holds. A
password change renews the Session for 7 days, and starts its 30 days
again. Why: a Session forgotten on a shared computer ends within a week,
while an account in daily use never sees the shorter limit, and the cap
bounds how long one Session token lasts however it is used. The Audit
Trail reads a Session's moved expiry from `session_extensions`, because
its own entries are never edited.

### API-73 An API token carries `import` and `export` capped by the account's, never `delete`, and never signs in

An **API token** is a named credential an account makes for a program, with
the scopes the person chose from `import` and `export`, capped by the
account's own. The cap holds on every request, when the token is made, and
when it is listed, so the list shows what the token may do now: a scope the
administrator turns off later shows as off. A token never carries `delete`: permanent deletion is a
person's act, and a leaked or faulty program must not be able to empty an
archive. A token never signs in and never browses. It is ended by the person
or the administrator revoking it (`DELETE /v1/accounts/{id}/api-tokens/{token_id}`,
with a session) or by its expiry, never by the program holding it. The
administrator makes no token, and a token row on the administrator's account, written by
hand or restored with a database, answers the same `401 Unauthorized` as a
token the server never issued. Why: no token acts as the administrator
(`docs/adr/0008-the-administrator-holds-no-messages.md`), and the refusal sits in
the token lookup every route goes through, so a route that forgets to
refuse the administrator cannot break it.

### API-74 `GET /v1/session` answers whose credential the caller holds

`GET /v1/session` answers whose credential the caller holds — the account's
id and username — for a session or a token. Why: a program holding a token
needs to know which account it writes to before it starts, and push and export
label their work with it. `DELETE /v1/session` refuses a token with
`403 Forbidden`, because a token is not a Session and a `204 No Content`
would say something ended when nothing did.

### API-75 A Media Link reads one asset, its Preview and its Thumbnail for an hour, with no header

A **Media Link** reads one asset, its Preview and its Thumbnail, in the
account that made it, with no `Authorization` header. A Session makes it
with `POST /v1/assets/{sha256}/media-links`, which answers the URLs to
load: `/v1/assets/{sha256}?media_link=…` and its `/preview` and
`/thumbnail` twins. It is open for
one hour, and ends sooner when the Session that made it ends: by logout, a
new login, a password change, or the Session's expiry. A server restart
ends every Media Link too. Only `GET /v1/assets/{sha256}`,
`GET /v1/assets/{sha256}/preview` and `GET /v1/assets/{sha256}/thumbnail`
take one, and a request that sends
`Authorization` is judged by the header alone. A link that does not open
the asset answers `401 Unauthorized` with `media-link-invalid`, because its
remedy is a new link rather than a new login.
Why: a media element (`<img>`, `<video>`, `<audio>`) loads its own `src`
and cannot send a header, and a video that streams must be loaded by the
element itself (`docs/architecture/rules/media.md`, MEDIA-1). The desktop app's
download of an original takes one too. The app copies the answer to the
file the person chose as it arrives, so a video of hundreds of megabytes is
never held whole in the window or sent to the app as bytes. The window holds
the Session and the app does not, and a link that reads one asset for an
hour reaches far less than the Session token would.
How: the value is `<account_id>.<expires>.<signature>`, an HMAC-SHA256
under a key the server makes when it starts and never writes down, over
the account, the asset's fingerprint, the expiry and the hash of the
Session token that made it. The server stores nothing: it checks a link by
signing the same terms with the Session the account holds now, so a link
for another asset, another account, a later expiry or an ended Session
fails the same check. The server's log shows `media_link=[hidden]`,
because a credential is never logged. Every answer the three asset
routes give once the credential is accepted carries
`Cache-Control: private`, because the bytes are one account's
attachment, and one read with a Media Link carries `private, no-store`:
that request sends no `Authorization` header, so a shared cache in front
of the server would otherwise keep the answer under the link's URL and
serve it after the hour or the Session has ended. A problem such as
`404 Not Found` or `416 Range Not Satisfiable` carries it too, because a
cache may keep a `404 Not Found` and hide a Preview made later.
Why an hour: a phone video is watched and sought in within minutes, and an
hour leaves room for a long one; a link copied out of the page stops
working soon after. The web app makes a new link whenever it opens an
attachment again.
Rejected: the Session token in the query string. It would write the
credential that reaches every message into URLs, the page and any log on
the way, where a Media Link reaches one asset for an hour.
Rejected: a cookie. The desktop app's page is served from
`tauri://localhost` and its server answers at `http://127.0.0.1:8080`, so
every request is cross-site, and a browser sends a cookie on a cross-site
request only with `SameSite=None; Secure`, which needs HTTPS. Docker and
the desktop app must work one way.
Rejected: fetching the file with the header and handing the element a
blob. The whole file loads before the first frame, which is what streaming
removes.
Rejected: a stored link, a row per link. It is a row for every attachment
opened and a table to prune, and the check reads the Session row either
way.
Rejected: one link per account for all its assets. A link copied out of
the page would read every attachment the account holds.

### API-76 Browse routes take a Session only

Browse routes (conversations, messages, contacts, groups, tags, saved
searches, search fields) take a session only. A token is refused.

### API-77 Imports and asset writes need the `import` scope

`POST /v1/imports` and everything under a run, and every asset write, need
the `import` scope on either credential.

### API-78 Exports and `HEAD /v1/assets/{sha256}` need the `export` scope

`POST /v1/exports` and everything under a run, and
`HEAD /v1/assets/{sha256}`, need the `export` scope on either credential. A
program with an export token reads messages only through an Export Run it
started, so every read of message data by a program leaves a record.

### API-79 `GET /v1/assets/{sha256}` takes any Session, a token with `export`, or a Media Link for that asset

`GET /v1/assets/{sha256}` takes any session, a token with the `export`
scope, or a Media Link for that asset. Why: a person looking at a photo in their own conversation is not
exporting it, so an account whose `export` permission is off still sees its
attachments, as it still reads its messages. A token has no screen to show
bytes on; fetching them with one is taking them out, which is what the
`export` scope decides.

### API-80 A Preview and a Thumbnail are read under the same rule as their asset

`GET /v1/assets/{sha256}/preview` and `GET /v1/assets/{sha256}/thumbnail` are
read under the same rule as the asset they were made from (API-79), by the same
account and nobody else. Why: a Preview and a Thumbnail are the attachment's
content as much as the original is, so a caller who may not read one may not
read the others, and the administrator reads none.

### API-81 Only an account's Session makes a Media Link

`POST /v1/assets/{sha256}/media-links` takes an account's Session only.
Why: only a screen has a media element to put a link in, or a download for
the desktop app to make, and a program sends its token in the header. The
administrator holds no attachment to read.

### API-82 `HEAD /v1/assets/{sha256}` also accepts the `import` scope

`HEAD /v1/assets/{sha256}` also accepts the `import` scope: a program that
can only push may ask whether an asset exists, and may not read it.

### API-83 Permanent deletion needs a Session, never a token

Permanent deletion (`DELETE /v1/conversations/{id}`,
`DELETE /v1/contacts/{id}`, `DELETE /v1/trash`,
`DELETE /v1/accounts/{id}/messages`, `DELETE /v1/accounts/{id}`) needs a
session: the account's own with the `delete` permission, or, for an
account's messages and for the account itself, the administrator's. A token is
refused whatever its scopes.

### API-84 The Demo Account is refused every route that needs the `import` scope, permanent deletion, an address book load and making an API token by its id, whatever its permission row says

The Demo Account is refused with `403 Forbidden` and
`demo-account-protected` by its id, whatever its permission row says, on
every route that needs the `import` scope or the `delete` permission, on
`POST /v1/contacts`, and on `POST /v1/accounts/{id}/api-tokens`. Its
profile reports `export` and neither `import` nor `delete`, from the same
id. Why: it has no password, so its limits must not rest on a row
(`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`).

### API-85 An account may do everything with its own messages unless the administrator limits it

An account may do everything with its own messages, deleting them and
itself included, unless the administrator limits it. Deleting an account deletes
every message it owns, so an account whose `delete` permission is off
cannot delete itself either: it is refused with `403 Forbidden` and asks the
administrator, who can. No separate permission for closing an account exists,
because one that allowed it without `delete` would undo the administrator's limit.

### API-86 `/v1/accounts/{id}` is read and written by the administrator or by that account

`/v1/accounts/{id}` and everything under it is read and written by the
administrator or by that account. `POST /v1/accounts` takes the
administrator's Session only, and the `Location` it answers names a row the
administrator may read.

### API-87 An account's API tokens are listed and revoked by the account and the administrator, and made and renamed by the account alone

An account's API tokens are listed and revoked by the account and by the
administrator, and made and renamed by the account alone. The administrator's list holds
each token's id, label, permissions, creation, last use, expiry and state,
and leaves out `token_hint`. Why: the administrator must be able to end a
credential that has leaked, and a token's label and permissions are not
message content (`docs/adr/0008-the-administrator-holds-no-messages.md`). The
masked hint is part of the secret, and the administrator never reads a secret.
Making a token answers with its secret, so the administrator makes none, and a
token is the account's own name for its program, so the administrator renames none.

### API-88 An account's history is read under the account, with no permission asked

An account's history is read under the account:
`GET /v1/accounts/{id}/imports`, `GET /v1/accounts/{id}/imports/{import_id}`
and `GET /v1/accounts/{id}/exports`, beside `GET /v1/accounts/{id}/storage`.
They ask who is calling and no permission, so the administrator reads them, and so
does an account whose `import` or `export` permission is off. `/v1/imports`
and `/v1/exports` are the pipelines' routes: they ask for the permission,
which the administrator's session never carries, and a program's token reaches only
them. Each pair answers from one function, so the two lists cannot differ.
Which contacts a run created is content, so `/v1/imports/{id}/contacts` has
no twin under the account.

### API-89 The Audit Trail is read, never written, through a route

The Audit Trail is read at `GET /v1/audit-trail`, every account's entries,
which is the administrator's alone, and at `GET /v1/accounts/{id}/audit-trail`, the
entries about one account, which the administrator and that account read. Both answer
from one function, so the two lists cannot differ. No route writes, changes
or deletes an entry: the server writes one as part of the act it records.
Why: an account holder reads what the administrator did to their account, and a
record its subject or the administrator could edit would be no check on either
(`docs/adr/0020-the-audit-trail-outlives-the-account.md`).

### API-90 A deleted account's entries and runs keep the id of its `account_deleted` entry

A deleted account's entries and runs lose its account id, and each keeps
the id of the account's `account_deleted` entry, which is the deleted
account's id on the wire. `GET /v1/audit-trail/deleted-accounts` lists the
deleted accounts with those ids and usernames, and
`GET /v1/audit-trail?deleted_account_id=` narrows the administrator's list to one.
Both are the administrator's alone. Why: the account's own id names nothing once it
is gone, and the Demo Account is deleted and made again under one id, while
a username passes to a later account, deleted or live.
Rejected: narrowing by the username the entries keep, which reads two
accounts that held one username as one, and takes in the logins refused for
the username while no account held it.

### API-91 The account reads its own runs in full, and the administrator reads a run as a type of its own that carries no content

The account reads its own runs in full, a list's rows less what
API-44 keeps out of a row. The administrator reads each run as an
`AdminImportRun` or `AdminExportRun`: the source, mode, tool, times,
outcome and counts, with the counts an import's summary reported and how
many issues it recorded, and for an export only which form its scope took.
The times are the run's own, when it started and ended; the administrator reads
nothing of the backup an import read, neither the file the desktop app
recorded (`source_fingerprint`) nor when the backup was made
(`backup_taken_at`), because both say when and from what the account's
phone was backed up. The administrator's read of one run adds the attachments it
wrote whose Thumbnail or Preview the media pass could not make, each by
its file name, type and size with why each version was not made, and their
count by kind of failure: an attachment's name, type and size are what
ADR 0008 lets the administrator read, and the reason is what ffmpeg said about the
file or a phrase of the server's. Nothing of the message or conversation
the attachment belongs to comes with it (#2636).
Why: a staging summary lists the addresses of everyone in the backup, an
issue names its conversation's file, a note names a file or an address, and
an export's query is a search over
the account's messages, all content under
`docs/adr/0008-the-administrator-holds-no-messages.md`. The administrator's view is a type
of its own rather than the account's with fields removed, so a field added
to a run reaches the administrator only when someone adds it to that type.

### API-92 `GET /v1/server` and `POST /v1/server/setup` take no credential, and the settings and storage are the administrator's

`GET /v1/server` and `POST /v1/server/setup` take no credential.
`GET /v1/server` reports the Message Crate's state, `setup` until the
administrator has a password and `ready` after. `POST /v1/server/setup` sets
that first password and answers `409 Conflict` (`state-conflict`) once the
Message Crate is ready.
`/v1/server/settings` and `GET /v1/server/storage` are the administrator's: the
storage totals sum every account, and no account holds more than its own.

### API-93 The server's log is the administrator's alone

The server's log, `GET /v1/server/log-lines`, `GET /v1/server/log-files`
and `GET /v1/server/log-files/{id}`, is the administrator's alone. Why: it is every
account's requests in one stream, about the installation the administrator runs,
and a line never holds content, so the administrator may read all of it
(`docs/architecture/rules/server-log.md`).

### API-94 The attachment size limit is a server setting the administrator may set to any value of 1 byte or more

The attachment size limit is a server setting, `asset_max_bytes` in bytes,
and the settings row is the only place it lives. The administrator reads and
changes it at `/v1/server/settings`; `GET /v1/server` reports it to any
client. Why: a program that uploads has to know the limit before it
prepares attachments, and it holds an account's credential, never the
administrator's. A change is `422 Unprocessable Entity` only when it is zero or
more than the database can hold, and an accepted one holds from the next
upload, because every upload reads the setting. The part size a multipart
upload is told is the configured one or the limit, whichever is smaller,
worked out when the upload starts. Why: the limit is the administrator's to set,
and no value of it, and no edit to the config file, may leave a server
that refuses to start or an upload that cannot complete. Rejected:
refusing a limit below the configured part size, and stopping `serve`
when the config file's part size is above a stored limit.

### API-95 The attachment size limit holds the attachment uploads only

The attachment size limit holds the attachment uploads only:
`PUT /v1/assets/{sha256}` and the start of a multipart upload. Every
other route has a body cap fixed in the code. Why: the administrator may set any
limit of 1 byte or more, and a limit that held every body would refuse
the login and the settings change that raise it again.

### API-96 An import batch body is held to 16 MiB, and a stalled one frees its slot

An import batch body is held to 16 MiB, the most the desktop app's Upload
packs into one (`MAX_IMPORT_BATCH_BYTES` in `api-types`, the
one figure both sides read), and a larger one answers
`413 Payload Too Large`. Why 16 MiB: the server saves a batch in one write
transaction, and every other writer, a login included, waits 15 seconds
for the write lock before it answers `database-busy`. A full 16 MiB batch
saved in under 5 seconds in a timed release build. A 64 MiB batch held
the lock four times as long
([#2062](https://github.com/messagecrate/message-crate/issues/2062)).
A batch takes its import slot before it reads its
body, and writes the body under the Data Directory
(`data_dir/.import-batches/`), which `serve` empties as it starts. A body
that goes `[server] body_stall_secs` (60) with no byte arriving, or is
still arriving `body_total_secs` (300) after the read started, ends the
read with `408 Request Timeout` (`body-stalled`), which frees the slot.
An attachment body and a part of one are held to the same two limits,
though they take no slot, because each holds a connection and a file
while it reads, and so is every other body
(API-65). At 16 MiB, a full batch arrives inside 5 minutes at about
0.45 Mbit/s. A multipart part is 16 MiB by default (`[server]
asset_part_size`), and the Upload sends an attachment over 16 MiB in parts
(`MAX_PROXY_BODY_BYTES` in `import`), so an attachment
arrives at the same speed a batch does. Why: the server accepted eight
times what its client sends, and held a batch's whole text in memory
beside its parsed records (#2075); every batch waiting for a slot held its
body on disk (#2095); and that disk was the operating system's temporary
directory, often small or held in memory, where nothing removed what a
stopped server left (#2123). Only two slots exist, so without the bounds
two clients that went to sleep part-way through a body, or sent a byte
now and then, would stop every import on the server. A 10-minute limit for
a whole body held a slot that long
([#2897](https://github.com/messagecrate/message-crate/issues/2897)).

### API-97 `GET /v1/server` states the two body time limits

`GET /v1/server` states the two body time limits, in seconds, as
`body_stall_secs` and `body_total_secs`, to any client. The desktop
app's Upload reads them once it has logged in, and waits for the answer to
each batch, attachment or part for `body_total_secs` plus 5 minutes for
the server's wait for a slot and its save. Why: the limits are config
keys, and an Upload that kept its own copy, as it once kept a 10-minute
timeout written beside the server's, drifts from them. They are stated
there, not in the answer that starts an Import Run, because the desktop
app's web screens start the run and the Upload only posts into it, often
after a restart.

### API-98 `/v1/server/settings` reports the database's journal mode

`/v1/server/settings` reports the database's journal as `journal`: the
journal mode SQLite runs the database in, `mode`, read when the request
is answered, and `wal_error`, the error SQLite gave when the server asked
for write-ahead logging as it started, in SQLite's words. `journal` is not
a setting, so it sits in an object of its own beside the settings, and
the administrator cannot change it. `GET` and `PATCH` answer it, so the answer to
a change replaces the one the screen read without losing it. Admin Home
says so when the mode is not `wal`, or it cannot be read and SQLite gave
an error, and names that error. A `wal` mode hides the notice whatever
`wal_error` says, because the error is from the start and the file may
have been switched to write-ahead logging since. `mode` is `null` when it cannot be read, and the route still
answers. `wal_error` is `null` when SQLite gave no error: it turned
write-ahead logging on, or kept another mode without one. Why:
in any other mode every page waits while an import writes, and a warning
in the server's log was the only sign of it (#2201). Neither is on
`GET /v1/server`, which answers anyone: that route carries only what a
client needs before it logs in, and how the database is stored is not
that (#2703). SQLite's error goes to the administrator alone, as the server's log
does (#2691).

### API-99 Free text a route stores has a cap of its own

Free text a route stores has a cap of its own, stated in the field's
OpenAPI description: a name a person types (an account's display name, a
contact's name) at most 200 characters, an identity address it links at
most 320, an Import Error's `item` and `reason` and an Import Run note's
`item` and `text` at most 2,000 each, the `tool` and `device_id` a new
Import Run names at most 64 each, its `run_dir` at most 4,096, the
`PATH_MAX` of Linux, and each JSON value an Import Run stores as it is
created at most 65,536 bytes. The value is trimmed first, except a path,
whose spaces at either end are part of its name, and one over its cap is
`validation-failed`, never cut short. Why: the value is stored, shown on
every list that holds it, and written into the Audit Trail and the
server's log, so the 32 MiB body cap is no bound on it (#2183). An address
given to unlink or to find a row is not held to the cap, because it names
something already stored, and neither is an address book load, because a
file Export wrote must load back whatever an import stored.

### API-100 A password is at most 128 characters

A password is at most 128 characters wherever a request sends one: a new
account's, a login's, a new or current password on a change, and the
administrator's on setup. It is counted in characters, as a username is, never
trimmed, and has no minimum length but the administrator's one character. One
over the cap is `validation-failed`, and the OpenAPI description and
`maxLength` state it. Why: people count characters, not bytes; Argon2
reduces a password to a fixed size before its costly work, so the cap only
keeps a client from sending a huge body as a password, and 128 leaves
room for any generated password or passphrase, over the 64 NIST SP 800-63B
asks a verifier to allow (#2996).

### API-101 A multipart part is held to the part size its upload was told

Each part of a multipart upload is held to the part size its upload was
told when it started, which the upload's manifest records, and not to the
limit as it is now. Why: an upload in progress keeps the limit it started
with. Lowering the limit mid-upload otherwise refused every remaining part
with `413 Payload Too Large`, and the push failed the conversation (#1179).

### API-102 A part's body is written to disk as it arrives, never held whole

A part's body is written to a temp file in its upload's directory as it
arrives, and the server holds one buffer of it at a time, as the HTTP
library hands the body over, never the whole part. Once all of it is in,
a part of exactly its size takes the part's name and the manifest
records it; a part of any other size is refused and its temp file
removed. Why: a part read whole into memory held a whole part size
(`[server] asset_part_size`) of the server's memory per request, and
nothing limited how many requests one account with the import permission
sent at once, so it could take the server's memory from every other
account (#2114). With the parts on disk, a part request costs the server
the memory of one buffer of the body as the HTTP library hands it over,
whatever the client sends. The part size no longer sets it, so the server
sets no limit of its own on parts in flight, and the desktop app's 64
uploads at once cost 64 such buffers.

### API-103 An account may have at most 128 multipart uploads open at once

An account may have at most 128 multipart uploads open at once. The start
of one more answers `409 Conflict` (`state-conflict`), with `detail`
naming the limit, until one of them is completed, aborted, or abandoned.
Why: an open upload keeps its parts on the disk every account shares, and
with no cap one account could leave enough of them to fill it (#2176). It
is `409 Conflict` and not `422 Unprocessable Entity` because the account's
state refuses the start, and no rewrite of the request gets past it. The
number is twice the uploads the desktop app runs at once, 16 attachments in
each of 4 conversations, so an import goes as fast as the app can send it.

### API-104 An upload is abandoned once nothing has been written to it for the body stall time

An upload, multipart or a single-request body, is abandoned once nothing
has been written to it for `[server] body_stall_secs` (60) and no request
to it is running. A start that finds the account at the limit removes the
account's abandoned uploads first and counts again, and `serve` removes
every account's as it starts and each account's when its Import Run ends.
Why: an Upload the app's close or a crash stopped leaves up to 64 uploads
open, which nothing resumes, because the client keeps an upload's id in
memory only. With a day as the limit, two such stops held the account at
the limit, and the next Upload failed every attachment sent in parts for up
to a day (#2652). The stall limit is safe because a live upload never goes
that long between requests: the app sends each part as soon as the one
before it is answered. A request still running keeps its upload however
old its manifest is, because a part streams into a temp file of its own
and the manifest records it only once all of it has arrived, and a
completion writes no manifest. Only `serve` removes them,
not `process-assets` or `imports discard`: those commands may run beside
`serve` and cannot see its requests.

### API-105 The credential names the account, and no route takes an `account=` parameter

The credential names the account. No route takes an `account=` parameter. A
Media Link is a credential, so its `media_link` parameter names the account
inside its signed value and is not a parameter of that kind.

### API-106 No `/v1` route answers with a redirect, and the web app follows none

No `/v1` route answers with a redirect, and the web app refuses to follow
one: every call it makes with the `Authorization` header passes
`redirect: "error"` to `fetch`, so a redirect fails as a network error and
reads as an unreachable server. Its probe of `/health` refuses one too,
though it sends no credential, so the server status reads Disconnected
beside a login that fails, never Connected. Why: a redirect on `/v1` can
only come from something in front of the server, such as a misconfigured
reverse proxy, and following it to another host could carry the Session
token there. Current browsers drop `Authorization` on a cross-origin
redirect, but the Fetch standard gained that rule only in November 2022
([whatwg/fetch#1544](https://github.com/whatwg/fetch/pull/1544)), and the
desktop app runs in whichever WebView the system has (WebKitGTK, WKWebView,
WebView2). A visible failure is better than a token sent somewhere else. The desktop app's own Rust client drops the
header on a change of host already.

### API-107 A refused credential waits a short, fixed time, and nothing counts attempts

A refused credential waits a short, fixed time before the server answers. A
refused login (`POST /v1/session`) and a wrong `current_password` sent to
`PUT /v1/accounts/{id}/password` or `DELETE /v1/accounts/{id}` each wait
about a second (`REFUSAL_WAIT` in `crates/server/server/src/credentials.rs`),
then answer `401 Unauthorized` (`invalid-credentials`). Every refused login
gets the same answer: a username nobody holds, a wrong or empty password, a
disabled account. The request gives its database connection back before it
waits, so refusals waiting to answer do not hold the pool. A login with the
right password, and a right `current_password`, do not wait. Nothing counts
attempts: no route answers `429 Too Many Requests`, and nothing locks an
account out.

Why: the wait is basic common sense, so a wrong password costs a moment
before the next try. It is not a defense against password guessing from the
internet. A Message Crate reachable from the internet belongs behind a proxy
with TLS and a login of its own, and what the server itself must do there is
a separate design (#2894).

Rejected: counting attempts, per account, per username or for the whole
server, and answering `429 Too Many Requests` past the count (#2893). A
count per account let anyone who knew a username lock its holder out by
sending wrong passwords. A count per username needed timing machinery so
that it would not tell a stranger which usernames name an account, and kept
a count for every username tried, so a script trying new names grew it
without end (#2885).

### API-108 A refused login writes a warning naming the account, never the username typed

Each refused login writes a warning to the server's log with the account's id
(none for an unknown username), the reason and the connecting app. The
username typed and the password are never written
([The server's log](server-log.md)).

## Runs

### API-109 A run is recorded for good, with what started it, and outlives its account

An Import Run and an Export Run are recorded permanently, whether they
completed, failed or were canceled, and outlive the account that ran them, as
the rest of the Audit Trail does. Each run records what started it: a Session
with the app it named, or an API token by its label and hint as they were
then. There is no sessionless import and no unrecorded export.

Deleting an account closes each of its runs still running, Import Run and
Export Run alike, as `canceled` at the time of the deletion, before the run
is kept without the account. Why: the account's Sessions and API tokens end
with it, so no client is left to close the run, and a record kept as
`running` would say that work is still under way
(`docs/adr/0020-the-audit-trail-outlives-the-account.md`).

### API-110 A run's settings are stated once on creation, and `complete`, `discard` and `cancel` are the client's ways to close it

A run's settings are stated once on creation, never per batch or per page.
`complete`, `discard` (imports) and `cancel` (exports) are the client's ways
to close a run. A run is also closed as `canceled` by something other than
its own `complete`, `discard` or `cancel`. Starting an Export Run closes the
account's running one, and the server closes an Export Run no page of which
was read for an hour (API-119). Deleting an account closes each of its
running Import Runs and Export Runs (API-109). The server's
`imports discard` command closes an account's running Import Run, such as
one a killed import left open.

A client still sending to a run closed by a new Export Run, by the server
after an hour with no page read, or by `imports discard` gets the
`409 Conflict` every finished run answers (API-112). A request that arrives
after its account is deleted fails its credential before the run is read,
and answers `401 Unauthorized` (API-28).

### API-111 `complete` states the outcome, and a run an error stopped is `failed`, never canceled

`complete` states the outcome in its body, as `status`: `completed`,
`completed_with_issues` or `failed` for an Import Run, and `completed` or
`failed` for an Export Run. An Export Run's `cancel` is for a run the person
gave up. A run an error stopped is closed with `complete` as `failed`, never
with `cancel`. Why: the Audit Trail and the export history show `canceled` as
the person's own choice, so a failed run recorded that way lies the same way
a canceled run marked completed would (#2069).

### API-112 A finished run answers `409 Conflict` to every change

A run that has finished answers `409 Conflict` to `complete`, `discard`,
`cancel`, a batch, and a change of stage; its record is never rewritten. Its
outcome is stated once, as `status`. Why: the finished record is the history
the person reads, and a canceled run marked completed afterwards would lie. A
program unsure how a run ended reads it with `GET` rather than repeating the
call.

Rejected: a repeatable `complete`, answering `200 OK` when the run already
ended the same way. It makes one call safe to retry at the cost of a second
rule, and `GET` already answers the question a retry is asking.

### API-113 An Import Run's stage moves only forward

A running Import Run's stage moves only to the next stage in the order a run
passes through (`parse`, `write`, `staging_review`, `media`, `media_review`,
`upload`), from `staging_review` straight to `upload` (the move a client makes
when the import has no Media Stage), or to the stage it is already at. Any
other change of stage, backwards or skipping a stage, answers
`409 Conflict` `state-conflict` naming both stages, and the run keeps its stage
and summary. Why: the next visit resumes a run at the stage the server holds,
so a client that sent a wrong stage would have the run resumed at work it
already did (#1227).

### API-114 Every Import Run opens at `parse`, and `POST /v1/imports` takes no stage

Every run opens at `parse`, and `POST /v1/imports` takes no stage: a `stage` in
its body is ignored like any other field the request does not name. Why: a run
opened later in the order would be resumed at work nobody did (#2638), and a
field with one valid value carries nothing (#2858).

### API-115 A running run with no stored stage is refused any change of stage

A running run with no stored stage is refused any change of stage with
`500 Internal Server Error`. Why: the server never writes such a row, so there
is no stage to check the move from, and passing it unchecked would let any
move through (#2638).

### API-116 An Export Run's scope is one of three forms, stored as given

An Export Run's scope is one of three forms, stored as given: everything the
account holds; a query in the search language; or picked `conversation_ids`
and `message_ids`. The record holds what was asked for and how much matched,
never what the messages said.

### API-117 A `query` scope names the list its query is for

A `query` scope names the list its query is for, in `list`, and has no
default: `{"kind": "query", "list": "conversations", "q": "messages:>100"}`.
`messages` compiles `q` on the Messages list and hands over the messages it
matches. `conversations` compiles `q` on the Conversations list and hands over
every message of each conversation that list shows for it, as opening each
one would: the list's own treatment of the Trash decides which conversations,
and a duplicate message is left out.
Why: each list has words the other does not (`messages:` on Conversations,
`from:` on Messages), and for a word both have, the Conversations list shows
whole conversations where the Messages list shows single messages. A person
who exports the conversation list they are looking at expects those
conversations, so the scope must say which list was meant.
Why a field, when API-15 makes a choice between two lists a path segment:
that rule is about which resource a route reads. Here the resource is one, the
Export Run, and the list is part of what the run was asked for, stored with it
and read back.

Rejected: guessing the list from the words in the query. A query made of
words both lists have compiles on either and means something different on
each.

### API-118 An Export Run is a snapshot taken when it is created

An Export Run is a snapshot taken when it is created. In the transaction that
records the run, the server lists the ids of the messages the scope matches,
each at a numbered place (oldest first), and computes the four counts
(messages, conversations, distinct attachments, bytes) from that list.
`GET /v1/exports/{id}/messages` pages the list, never the scope again, and
`complete` or `cancel` deletes it.
Why: re-running the scope for every page let an import, a trash or a new day
between pages move the offsets, so pages skipped or repeated messages and
`import:last` or a relative date could mean something else by the last page.

- A page's `total` is always the run's `message_count`. `offset` and `limit`
  address places in the list, and `sort=-date` counts them from the end.
- A message that matched at creation is handed over even if its conversation
  is trashed afterwards, because the run asked for it when it could still be
  read. A message imported afterwards is not handed over.
- A message deleted permanently afterwards leaves its place empty: its page
  holds fewer than `limit` items, and the places after it do not move. A
  client steps `offset` by `limit` until it reaches `total`, never by the
  items it got, and never stops on a short or empty page.

### API-119 An account has at most one running Export Run: a new one replaces it, and an hour with no page read closes it

An account has at most one running Export Run, held by a partial unique
index on `exports(account_id) WHERE status = 'running'`, as for Import Runs.
`POST /v1/exports` replaces the account's running run: in the transaction
that records the new run, the running one is closed as `canceled` and its
list deleted. A start is never refused because a run is open. The server
also closes as `canceled` a running run with no page read for an hour,
counted from its last page or, before the first, from its start; it looks
once a minute, and when it starts. A page past the end of the list hands
over nothing and still counts as a page read, so the desktop app reads one
every ten minutes while it fetches the run's Assets, which read no page. A
client still reading a run closed either way gets the `409 Conflict` any run
that is not running answers, and the desktop app's Export fails with a
sentence saying it was canceled. Each closure is an `export_run_closed` entry
in the Audit Trail, with the reason `replaced` (by the person starting the
new Export) or `idle` (by the server), naming the run.
Why: every running run holds one row per message it matched, written while
holding the database's write lock. With no limit, a program with the
`export` scope could start runs in a loop and grow that table by the
account's whole message count each time, and a client killed mid-Export
left its list behind for good (#2080). Replacing rather than refusing keeps
a person who restarts an Export from being locked out by the run their
killed app left.

## The reference

### API-120 The reference is generated from the handlers and checked in

The generated reference, `docs/src/assets/openapi.json`, is produced from the
handlers by `message-crate-server dump-openapi` and checked in; a test fails
when the two differ, and CI checks the web app's generated types against it.

### API-121 An operation's error responses are built from shared parts, never written by hand

An operation's error responses are built from shared parts, never written out
by hand. The credential a route accepts brings its `401 Unauthorized` and
`403 Forbidden`, and a Media Link brings `media-link-invalid`; a request
body brings `400 Bad Request`, `408 Request Timeout` (`body-stalled`),
`413 Payload Too Large`, `415 Unsupported Media Type` and
`422 Unprocessable Entity`; an id in the path
brings `404 Not Found` and `422 Unprocessable Entity`; every `/v1` route brings
`422 Unprocessable Entity` for a query parameter it does not declare; and every
`/v1` route that answers JSON brings `406 Not Acceptable` for an `Accept` that
names nothing JSON. `405 Method Not Allowed` is said once, in the document's
own description, because it answers a method no operation has. The handler adds
only what is its own, such as `409 Conflict` for a run in the wrong state, by
naming the problem type (`crate::problem::openapi`). Every error response is
declared as `application/problem+json`, names the problem types it can carry
(in its description and in `x-problem-types`), and has a description.
Why: every mismatch between the reference and the handlers that the September
2026 review found was in a hand-written list.

### API-122 An operation's summary is the first sentence of its handler's doc comment

The first sentence of a handler's doc comment is the operation's summary, and
the rest is its description.
Why: the summary is the operation's title in the reference's operation list
and in the generated client types, so it has to read as one line; everything
past the first sentence is description, and the cut keeps handler doc comments
free to say more without lengthening the list.

### API-123 A field's description sits on the field

A field's description sits on the field. utoipa writes the doc comment of an
optional field typed by a schema of its own inside the `oneOf` branch beside
the `$ref`, and the finished document moves it onto the `oneOf`. A `oneOf`
that is a schema of its own keeps a description on each branch, because there
it describes the branch.
Why: openapi-typescript reads no description inside a branch, so the web app's
generated type for such a field carried no doc, while the same field made
required kept it.

### API-124 Every field has a description

Every field has a description, written from the doc comment on its Rust
field, and a document rule fails on a field with none. The tag of an
internally tagged enum, such as `ExportScope.kind`, is the one field no Rust
code declares, so the finished document describes it from its values: each
form's tag names that form and the others.
Why: in October 2026, 59 fields of the reference had no description, and a
reader of the reference or of the web app's generated types had only the
field's name to go on.

### API-125 A rule that can be checked by walking every operation is checked that way, by one test

A rule that can be checked by walking every operation in the document is
checked that way, by one test, as `openapi/credential_matrix.rs` checks every
route's reach: the page shape and paging parameters on every list, a
`Location` on every `201 Created` that the credential which made it can `GET`,
a problem document on every failure, `401 Unauthorized` without a credential, a
refused unknown query parameter, `415 Unsupported Media Type` for a body
without an accepted `Content-Type`, `400 Bad Request` for a JSON body that is
not JSON, `406 Not Acceptable` exactly where the document lists it, a
successful `GET` in a media type its document declares, every property of a
success answer required and every one of them in what the route answers
(API-6), no body on a `HEAD` answer, `Cache-Control: no-store` on every answer
but those of an operation that lists its own (API-69), kebab-case paths and the
nesting depth. Why: a rule checked one route at a time is checked on the routes
someone remembered.

### API-126 The shared failures are checked by calling each operation into them

The shared failures are checked by calling each operation into them, and the
status and problem type the server answers must be ones the document lists.
Reading them back from the document proves nothing, because the shared parts
wrote them there from the same inputs the reading would use.

## Code

### API-127 A route group is one module named for the route's first path segment, with `_api`

A route group is one module named for the route's first path segment, with
`_api`: `contacts_api`, `conversations_api`, `imports_api`, `exports_api`,
`assets_api`, `search_fields_api`, `session_api`, `server_api`, `trash_api`.
Contact Groups and Message Tags, one shape served twice, share
`named_set_api`. A collection nested under a member is a submodule of that
group: `/v1/accounts/{id}/api-tokens` is `accounts_api::api_tokens`.
Why: a route's code is found from its URL without searching.

### API-128 A handler is named `verb_noun`

A handler is named `verb_noun`, with no `_handler` suffix. The verb is `list`,
`get`, `create`, `update` (`PATCH`), `replace` (`PUT`) or `delete`, or the
action's own verb: `list_contacts`, `get_contact`, `update_contact`,
`set_up_server`, `complete_import`.
A `HEAD` handler takes the method's own verb, `head`: `head_asset`.
A `POST` that reads is named for what it returns, as its route is:
`list_contact_summaries`, `get_address_book`.

### API-129 A wire type is named for what it is, for the part of a thing it answers, or for its action

A type on the wire is named one of three ways, and a reader can tell which
from the name:

- A thing the interface hands out is named for what it is, with no suffix:
  `Message`, `Account`, `ApiToken`, `Contact`, `Identity`, `ImportRun`. It
  keeps that name wherever it appears.
- A projection of such a thing, a type that answers part of it where the
  whole does not belong, is the thing's name with one word that says which
  part, and is written down here with its reason. `Summary` is the thing as
  a list answers it: `ContactSummary` is a contact's row in the Contacts
  list, and `ImportRunSummary` is an Import Run without its issues and
  notes, which `ImportRun` adds to it (API-44). `Admin` is the
  thing as the administrator reads it under another account: `AdminImportRun` and
  `AdminExportRun` (API-91), and
  `AdminImportRunSummary` is an `AdminImportRun` as the administrator's list answers
  it, without the attachments whose versions were not made. `NotMade` is a
  Thumbnail or Preview the server could not make, as the administrator reads it:
  `NotMadeAttachment` is an attachment that has one, by its file name, type
  and size and nothing of its message or conversation, because those are
  what ADR 0008 lets the administrator read of an attachment. The types it carries
  share the prefix: `NotMadeVersion` (why one version was not made),
  `NotMadeKind` and `NotMadeCount`.
- An action's input and output are named for the action:
  `VerbNounRequest` for a body sent in, `VerbNounResponse` for an answer that
  is not a thing (`CreateApiTokenRequest`, `DeleteMessagesResponse`), and
  `VerbNounQuery` for a query string (`ListContactsQuery`).

No other endings: no `Body`, `Payload`, `Input`, `Patch`, `Item`, `Info`,
`Detail` or `Row`. Why: these names become the web app's type names, and
`Message` next to `MessageResponse` would leave a reader asking whether they
are one thing or two.

### API-130 A wire type whose Rust name would clash takes the `Api` prefix in Rust and keeps its wire name

A wire type whose Rust name would clash with a shared crate's type, or with
the `db/` struct it carries whole, takes the `Api` prefix in Rust and keeps
its wire name through `#[schema(as = …)]`:
`ApiIdentityService` in `db/handles.rs` is `IdentityService` on the wire,
beside the conversation file's `ir::IdentityService`, and
`ApiServerSettings` in `server_api.rs` is `ServerSettings` on the wire,
carrying `db/server_settings.rs`'s `ServerSettings` and the database's
journal. Why: two types
under one bare name mean one or the other depending on a file's imports, and
those two read the same word differently.

### API-131 A handler holds no SQL, and neither does an import stage: every query lives in `db/`

A handler reads the request, checks the caller and shapes the answer; it holds
no SQL. Every query lives in `db/`, in the module for the table it is chiefly
about, and it is written for SQLite, the only database engine
(`docs/adr/0017-sqlite-is-the-only-database-engine.md` has the rule and its
reason). Why no SQL in a handler: a query written into a handler gets written
twice (the identity counts for contacts and for accounts were).

Does the rule stop at handlers? No. An import stage (`imports_api/staging.rs`,
`imports_api/promote.rs`) is not a handler, but it holds no SQL either: it
sequences its statements, logs them and keeps the counts, and the statements
are in `db/staging.rs`, the module for the staging tables. The dedupe every
Import Run runs as it ends (`dedupe.rs`) is a stage too: it hashes the content
keys and picks the copy shown, and the statements that read and write
`messages.content_key` and `messages.duplicate_of` for it are in
`db/dedupe.rs`. Why: the point of the rule is that the SQL for a table is
found in one place, and a stage that carried its own statements would be a
second place for its table's SQL: `staging_messages` for the import stages,
`messages` for the dedupe.

### API-132 A promotion statement belongs with the staging tables

A promotion statement reads a staging table and writes a production one in
the same `INSERT ... SELECT`, and it belongs with the staging tables rather
than the production one, because the staging tables have no reader but the
import while the production tables have many. The messages watermark and
count a promotion takes before it inserts live there too, for the same
reason: nothing but a promotion reads them.

### API-133 A handler takes the part of the server's state it reads, never `AppState` whole

A handler takes the part of the server's state it reads, never `AppState`
whole. The database is `State<SqlitePool>`, the configuration is
`State<Arc<Config>>`, and each other part is a sub-state that `server.rs`
cuts from `AppState` with `FromRef`: `ImportState`, `AssetState`,
`DemoState`, `RefusalWait`, `BodyTimeLimits`, `MediaLinkKey` and
`WalError`. The import routes take `Importer`, in
`imports_api/run_control.rs`, which holds what running and ending an Import
Run reads. The credential extractors (`AuthIdentity`, `FullAccess`, `Admin`
and the rest) read only the pool, so they work under any router state that
has one. Why: a handler's signature then names everything it reads, so a
reader sees its dependencies without reading its body. A change to the Demo
Account build or to `Importer` does not touch a handler that never takes
them.

### API-134 A `db/` function never returns `ApiError`

A `db/` function never returns `ApiError`. It returns `anyhow` or `sqlx`
errors when a statement is all that can fail, and an error type in `db/` when
the caller must tell outcomes apart (`SavedSearchError::NameTaken`,
`MessagePageError::Invalid`); the `From` that turns each into a problem type
is in `api_error.rs`. Why: the command line import and the Demo Account build
call `db/` too and speak no HTTP, and the answer a route gives for an outcome
is the handler's choice, not the storage layer's.

### API-135 A route whose answer is one `db/` read's result answers with the read's own struct

A route whose answer is one `db/` read's result, as the read returns it,
answers with the read's own struct. That struct derives `Serialize` and
`utoipa::ToSchema`, takes its wire name by the rules above (API-129, API-130),
and the handler hands it on unchanged: `list_saved_searches` answers
`Json<Page<SavedSearch>>` with the `SavedSearch` rows `db/saved_searches.rs`
reads. A struct that a client crate reads too lives in `api-types`, and the
`db/` read returns that struct: `ExportRun` from `db/exports.rs`, `Message`
from `db/conversation_messages.rs`. Why: one struct cannot drift from itself,
and a copy beside it would be a second place to change for every column. No
client needs the storage shape hidden from it, because no client is kept
compatible (API-137). A column rename that reaches the wire costs a rebuild and
nothing more.

Does a route group then never define a wire type of its own? It does, when the
answer is not one read's result as it stands. An action's answer is a
`VerbNounResponse`, such as `DeleteMessagesResponse`. A thing assembled from
several reads is defined in the route group: `Account`, `Contact`, and
`ImportRun`, which is an `ImportRunSummary` with the run's issues and notes.
So is a thing whose fields depend on who asks: `ApiToken` carries a `null`
`token_hint` when the administrator lists another account's tokens, and
`AdminImportRun` leaves out what the backup held. So is a thing whose stored
form is not its wire form: `ImportRunSummary` decodes the JSON text its row
holds in `form_json`, `summary_json`, `source_fingerprint` and
`source_identities` into JSON values, and answers `form_json` as `form` and
`summary_json` as `summary`. So is a thing no table holds, such as `Asset`,
`MediaLink` or `PhoneCountry`. A struct that repeats a `db/` struct's fields
and is filled from it field by field is none of these, so it is not written:
the `db/` struct goes on the wire instead.

### API-136 A test of a route's answer goes through the router and the shared fixtures

A test of a route's answer goes through the router, checks a failure with
`expect_problem` (status, `type` and `request_id`, not only the sentence), and
takes its database and account from the shared fixtures in `test_support.rs`.
A state the router cannot reach is the one exception: a row deleted between
the credential check and the handler's read, say, because the check refuses
the request first. Such a test serves the handler on a router of its own,
under `request_id::layer`, hands it the identity the check would have made,
and reads the answer over HTTP with `expect_problem` like any other.
A rule every route follows is tested once over the whole document, as above
(API-125), not again per route.

## Versioning and change

### API-137 `/v1` is a whole number in the path, with no compatibility promise behind it

`/v1` is a whole number in the path. There is no compatibility promise behind
it: endpoint names, request and response shapes, and stored formats change
whenever a better design is found, and breaking a client is an accepted cost.
No compatibility alias, deprecation window or version handshake is ever added.

### API-138 The server says which code it runs and an app says which code it is, and neither decides anything

The server says which code it runs, and an app says which code it is, and
neither decides anything. `GET /v1/server` carries the server's Build in
`version`, the Schema Fingerprint in `schema_fingerprint`, and the Message
Crate's id in `id`, which says which Message Crate answered, not which code
it runs. The desktop app
and the website send `x-message-crate-app` (`desktop` or `website`) and
`x-message-crate-version` (their Build) on every request; the server records
the pair on the account's session, rewrites it only when it changes, and shows
it to the administrator. A request that sends neither header, or sends them
malformed, is served and nothing is recorded, which covers curl, Swagger UI
and any program holding an API token. No route refuses, redirects or changes
its answer on account of either header: this is a record of what connected,
not a handshake. The Product Version is the only value an app or Admin Home
compares, and the HTTP interface has no version number of its own beyond the
`1` in the path.
