# The search language: rules

The constraints the search language must keep satisfying on every list. The
grammar, the values, each list's defaults and what every word means are in
[The search language](../search.md); this document is what a change must
not break. Each rule has an id, `SEARCH-n`, which code comments, tests and
reviews cite. A rule's id never changes. A rule that stops holding is
removed, and its id is not reused.

Each rule carries its reason. Why the language is one module with one
registry is
[ADR 0004](../../adr/0004-one-search-language-compiled-in-one-module.md).

## SEARCH-1 One word per concept

A concept has exactly one spelling and no aliases. Precision lives in the
value (`date:>2019`, `size:<500k`), never in paired words like `before:` and
`after:`. Why: two spellings for one idea are two things to document,
suggest, and keep in step, and a person cannot tell whether they mean the
same thing.

## SEARCH-2 A word is shared between lists only when it answers the same question on each

`group:Family` asks "is this in the Contact Group" on every list it is on,
reaching the contact from a conversation or a message. A word whose question
has no answer on a list is not on that list: `from:` is a Messages word,
because "who sent it" is a question about one message. A word that counts or
dates messages asks about the base row's own messages: on Conversations and
Messages those are the conversation's, and on Contacts they are the messages
the contact sent, so `last-message:` is when the conversation last had a
message on one list and when the contact last wrote on the other (#726). Why:
a shared word that answers different questions is read by its most natural
meaning, and on the other list that reading is wrong.

## SEARCH-3 A word the list does not have is refused

The query is refused whole, with a problem naming the word and the list, and
a "did you mean" only when a word on that list is within two edits. The
server never searches such a word as text and never drops it. The language
keeps no table of spellings it used to have. Why: a silently ignored word
returns rows the person did not ask for, and they cannot see why.

## SEARCH-4 On the Messages screen, a word only the other list takes is marked, not sent

The screen's search box serves the Conversations and the Messages list
through one switch. A word the list picked does not take and the other one
does (`from:` while Conversations is picked) is underlined in the box, and
the web app sends the list the query without it (#1561). It is the only place
a word is left out, and the person sees it left out, so it is not ignored
silently. Until both lists' words are known, a list with a `word:` in its
query waits rather than send a word the server would refuse. Leaving the word
out is the one rewrite of a query outside `crates/server/server/src/search/`:
`dropTokens` in `web/src/lib/searchQuery.ts` also drops a `not` that negated
the word and an `or`, `and` or pair of parentheses it leaves with nothing to
join, and keeps one that joined nothing before. Its results are in
`tests/fixtures/search/web-queries.txt`, which the server's tests parse, so a
change to the grammar that breaks the rewrite fails a test.

## SEARCH-5 A query only narrows

Sort order is a request parameter (`sort`), never a word. Why: a Saved Search
then holds only what to find, so it means the same thing on every screen that
runs it.

## SEARCH-6 A Saved Search is checked for what every list refuses, and for nothing else

Saving or editing one reads its query without a list
(`search::check_for_every_list`): a query over 2,048 bytes, a syntax error
such as a parenthesis or quote that never closes, more than 32 free-text
words, more than 64 parts, or more than 32 levels of parentheses and `not` is
refused with the sentence a list gives (#2184). No `word:` is looked up and
no value is read. A word one list lacks, a word no list has, or a value no
word understands is stored as written and refused when it runs. Why: a Saved
Search runs on more than one list, and the words differ between lists. The
limits and the grammar are the same on every list, so a query that breaks
them could never run anywhere.

## SEARCH-7 Relevance ranks by the positive free-text words

The Messages list's `sort=relevance` orders by the full-text index's `bm25()`
over the query's free-text words that are not behind `-` or `not`; field
words never rank. A query with no such word refuses `relevance` with
`validation-failed` rather than answering in another order, and `-relevance`
is refused too: best match first is its only direction. A message matched
only by an attachment's file name, or only by an earlier version of an edited
message, has no rank and follows every ranked one. The rank is read from the
index once for the whole search, never per row (#413). Why: a word that only
excludes says nothing about how well a message matches, and a silent fallback
to date would show an order the person did not pick.

## SEARCH-8 With no `sort`, the server picks the Messages list's order

A query with a positive free-text word is ranked best match first, and a
query with none, field words only or nothing, lists the newest message first.
Every page says which order it applied and which terms it ranks by, in
`search` (`http-api.md`, API-41), from `Expr::positive_text_terms`. A
`sort` the request names is applied as it is. Why: a search for words is
looking for the messages that hold them, and a search by fields alone is
browsing, where the latest come first. The choice depends on which words
rank, which only the search module, `crates/server/server/src/search/`,
knows, so the server makes it and the client reads it back rather than
parsing `q` itself (#1538).

## SEARCH-9 Compile is pure

Compiling reads no database and no clock. Anything that needs a lookup (the
last Import Run, a Contact Group by name) is a subquery, and today's date and
the account's time zone are inputs. Why: the same string compiles to the same
SQL, so tests can pin it, and a query never depends on the moment it was
parsed.

## SEARCH-10 `q` and `-q` split the list

`-q` is every row of the list that `q` does not match. A row with no value
for the word does not match `q`, so it matches `-q`: `-last-message:>=2022`
on Contacts includes the contacts who never sent a message. The one exception
is a word that lifts a default ([Lists](../search.md#lists)), where `q` and
`-q` together cover the lifted list. Positive words stay strict: a date word
matches only rows that have that date. Why: a row missing from both answers
is invisible to every search and cannot be explained (#698).

## SEARCH-11 An id that names nothing matches nothing, and is not refused

`group:#99` for a Contact Group that does not exist, or that belongs to
another account, matches no row, and `-group:#99` matches every row. Why: a
Saved Search that names a Contact Group later deleted must keep running
(#698).

## SEARCH-12 Every query is scoped to the logged-in account

The compiled filter is `AND`-ed with the base row's account, so no row of
another account can match. Why: an account's data is isolated from every
other account in the database.
