# What web-next has that web lacks

A comparison of `web/`, the product UI, with `web-next/`, the Next.js browse UI
kept in the tree for comparison, as both stood on 2026-10-24 (`main` at
`fc8ddb785`). It lists the functions web-next offers that web does not, so
each can be weighed for porting. Many differences are deliberate redesigns
in web, and the second section names the ones where web meets the same need
another way. A reader who wants only the gaps can stop after the first
section.

[`web-next.md`](web-next.md) is the earlier record of web-next's screens,
captured on 2026-09-05. Its feature inventory predates the changes listed
below and no longer describes web, so this document replaces it for the
comparison.

The screenshots are of both apps running against one server, logged in as
`demo2`, an ordinary account holding a copy of the Demo Data (83 contacts,
104 conversations, 63,225 messages), in headless Chrome at 1360 × 860 with
the dark theme. They live in [`2026-10-24/web/`](2026-10-24/web/) and
[`2026-10-24/web-next/`](2026-10-24/web-next/), named as
[`README.md`](README.md) describes. The Demo Account itself refuses
imports and deletes, which is why the screenshots use `demo2`.

Each finding was checked against the current source of both trees, and the
paths are given. Every write in web-next answers `501 Not Implemented`, as
`web-next.md` describes, so a web-next feature here is what its screen
offers, not something that works end to end.

## Functions only web-next has

The list runs from the most useful to the least.

### Selecting conversations: ctrl-toggle, shift-range, and bulk trash

Web's conversation list has a checkbox on every row and a select-all
(`web/src/screens/ConversationList.tsx:203`), and a selection feeds one
action, the Message Tags menu (`ConversationList.tsx:147`). Shift-click does
not select a range there. The shift-click below checked the two rows clicked
and nothing between them. Ctrl-click and cmd-click do not toggle a row
anywhere in web. A selection of conversations cannot be moved to Trash in
one step either: Trash is in each conversation's own "⋯" menu.

Web's contact list does select a range with shift-click
(`web/src/screens/ContactList.tsx:312`), and a contact selection feeds the
Contact Groups menu and Export.

web-next's shared selection model (`web-next/src/components/useListSelection.ts`)
gives every list the same behavior: shift-click selects a range, ctrl-click
or cmd-click toggles one row, Escape clears the selection, and Delete or
Backspace trashes it (`web-next/src/components/BrowseShell.tsx:1714`). It
applies to contact rows, group conversation rows, search results and
Trash. The "⋯" list menu offers "Delete group messages" and "Delete
contact" for the selection.

| web | web-next |
|---|---|
| ![Two conversations checked](2026-10-24/web/select-conversations-two.png) | ![Two contacts checked](2026-10-24/web-next/select-contacts-two.png) |
| ![Shift-click on conversations checks only the two rows clicked](2026-10-24/web/select-conversations-shift-click.png) | ![Shift-click selects the range](2026-10-24/web-next/select-contacts-shift-click.png) |
| ![Shift-click on contacts selects the range](2026-10-24/web/select-contacts-shift-click.png) | ![The list actions menu](2026-10-24/web-next/menu-list-actions.png) |

### Merging contacts

A contact with no name offers "Merge into…" in web-next's contact menu
(`web-next/src/components/BrowseContactCtxMenu.tsx:99-160`). Web has no way
to merge two contacts.

### Creating one contact by hand

web-next creates a contact from "New contact" in the list menu or the
contact menu, and from a group participant who has no contact yet
(`web-next/src/components/BrowseContactCtxMenu.tsx:71`, `ParticipantContactFormOverlay.tsx`). Web
creates contacts only in bulk, by loading an address-book CSV with Append
(`web/src/screens/settings/AddressBookSection.tsx:21`).

### Right-click menus

web-next opens a menu on right-click for a contact (New contact, Edit,
Merge into…, Labels, Delete contact), a search result (Delete message), and a
Trash row (Restore, Delete forever), in `BrowseContactRow.tsx:82`,
`SearchResultsList.tsx:187` and `TrashUnifiedList.tsx:120`. Web has no
right-click menu anywhere. Right-clicking a conversation or a contact shows
the browser's own menu.

![web-next contact menu on right-click](2026-10-24/web-next/menu-contact-right-click.png)

### "Photos & files" in a conversation

The "Photos & files" button narrows web-next's thread to messages with
attachments (`web-next/src/components/BrowseThreadPane.tsx:226`). Web has no
attachments-only view of a conversation.

![web-next thread narrowed to photos and files](2026-10-24/web-next/thread-photos-and-files.png)

### Reading a conversation one source at a time

web-next's source picker (Combined, iMessage, SMS Backup Restore, WhatsApp)
filters the thread to the messages from one import source
(`BrowseThreadPane.tsx:505-537`). Web's Sources drawer reports how many
messages each source holds and does not filter the thread
(`web/src/components/SourcesPanel.tsx`).

### Advanced search fields

web-next's advanced form (`web-next/src/components/AdvancedSearchForm.tsx:384-519`)
has fields web's form lacks: doesn't have the words, subject, has an
attachment, file type, file name, larger than and smaller than, sort, and
context. Its contacts tab adds group and direct message counts
(`AdvancedSearchForm.tsx:306-311`). Web's contacts tab already has the
Identity, first message and last message fields
(`web/src/components/advancedSearch/ContactsSearchFields.tsx:121,173,179`).
A "contacts with matching conversations" result mode, a contact row that
expands to its matching conversations, has no web counterpart. Web's Messages tab does give one row per matching
message, which web-next spells `group:none`.

This document does not establish whether the server's search words already
accept these fields. Porting a field starts with that check.

| web | web-next |
|---|---|
| ![Search suggestions with Advanced search](2026-10-24/web/search-popdown.png) | ![Advanced search form](2026-10-24/web-next/search-advanced.png) |
| ![Messages tab, one row per message](2026-10-24/web/search-messages.png) | ![One row per message](2026-10-24/web-next/search-messages.png) |

### Sorting contacts by phone, message count or group count

web-next sorts contacts by first name, last name, phone number, message
count or group count (`web-next/src/components/SortByMenu.tsx:154-158`).
Web sorts by first name, last name or Last Heard From
(`web/src/components/ContactSortMenu.tsx:5-7`), and Last Heard From is web's
own.

| web | web-next |
|---|---|
| ![Sort contacts menu](2026-10-24/web/menu-sort-contacts.png) | ![Sort contacts menu](2026-10-24/web-next/menu-sort-contacts.png) |

### Year filter and activity by year for group conversations

web-next's Group Messages list has a year filter and sorts by date, message
count or people (`SortByMenu.tsx:254-257`). Its details panel
shows a conversation's messages per year
(`web-next/src/components/BrowseDetailsInspector.tsx:169`). In web, group
conversations are in the one conversation list, and the conversation type
is a field of the advanced search. "Jump to" moves through a conversation by
year but does not filter a list.

| web | web-next |
|---|---|
| ![Jump to menu](2026-10-24/web/menu-jump-to.png) | ![Year filter menu](2026-10-24/web-next/menu-year-filter.png) |
| ![Sort conversations menu](2026-10-24/web/menu-sort-conversations.png) | ![Sort group conversations menu](2026-10-24/web-next/menu-sort-chats.png) |

### A year strip that follows the scroll

web-next's thread carries a strip of years that highlights the year on
screen as the thread scrolls (`BrowseThreadPane.tsx:350-380`). Web's
"Jump to" menu moves to a year, and nothing shows which year is on screen.

### Message preview in Trash

web-next shows a trashed conversation's messages beside the Trash list
(`web-next/src/components/TrashShell.tsx:640`). Web shows the trashed
conversation's facts but not its messages. Both delete forever. Web has
Delete per item and Empty Trash (`web/src/screens/TrashScreen.tsx:275,421,527`).

### Date and time formats

web-next's Appearance settings choose date and time formats, custom patterns
included (`web-next/src/components/DateTimeSettings.tsx`). Web sets a time
zone and takes its formats from the browser.

### Home dashboard

web-next opens on a Home screen with counts of messages, contacts, group
conversations and attachments, recent contacts, a message history panel,
and shortcuts to all contacts and to contacts with no messages
(`web-next/src/components/HomePageClient.tsx:44`). Web opens on the
conversation list.

![web-next Home](2026-10-24/web-next/home.png)

### Switchable list badges

web-next's Appearance settings turn the contact list's date range, direct
message count and group icon on or off
(`web-next/src/components/MessageBadgeSettings.tsx`). Web's rows show a fixed
set: initials and last heard date for a contact, date range and participant
count for a conversation.

### Navigation that collapses to an icon rail

web-next's whole navigation collapses to a rail of icons
(`web-next/src/components/AppShell.tsx:36`). Web collapses its navigation
one section at a time.

### "No password" at account creation

web-next's account creation form has a "No password" option
(`web-next/src/components/LoginScreen.tsx:29-38`). Web creates an account
with a username and password, then asks for the name and identities on the
onboarding screen.

## Needs web meets another way

These are web-next features that web covers by a different design. Each is
a fact about the two trees, not a gap to close.

| web-next | web |
|---|---|
| A contact row expands to its direct thread and its group conversations | The conversation list, and a search by participant |
| A "No messages" section and Home tile | The advanced search's "Never messaged" (`web/src/components/advancedSearch/ContactsSearchFields.tsx:208`) |
| A "Needs review" note on an ambiguous handle | A note on a number with no known country, and a country picker that fixes it (`web/src/components/IdentityTable.tsx:132-136`) |
| Import VCF, with categories mapped to labels | Convert the vCard file to Message Crate's CSV, then load it (`web/src/screens/settings/ConvertVcardFile.tsx`) |
| Labels on contacts | Contact Groups, with their own options menu |
| "Message Vault" title menu with Logout | Account menu with Settings and Log out |

Web has a set of features web-next lacks, among them Message Tags, Saved
Searches, the Unknown and No Contact Group pages, owner administration,
import and export in the desktop app, tapbacks, and a per-service message
style. Those are outside this document's question.

## Menus in both apps

The menus each app opens from a button, as found in the source.

| web | web-next |
|---|---|
| Sort conversations: Date or Messages, ascending or descending | Sort group conversations: Date, Message Count, People |
| Sort contacts: first name, last name, Last Heard From | Sort contacts: first name, last name, phone, message count, group count |
| Message Tags on a selection of conversations | Year filter on the contact and group lists |
| Contact Groups on a selection of contacts | "⋯" list actions: New, Import VCF, Export contacts CSV, Edit, Labels, Delete group messages, Delete contact |
| "Jump to": Newest and each year | Label options: Rename, Delete |
| Conversation "⋯": each participant (opens the contact), Sources, Move to trash, and Make a Contact Group in a group conversation | Title menu: Logout, Reset demo |
| Contact Group, Message Tag and Saved Search options: Rename, Delete | |
| Account: Settings, Log out | |

| web | web-next |
|---|---|
| ![Message Tags on a selection](2026-10-24/web/menu-message-tags.png) | ![Label options](2026-10-24/web-next/menu-label-options.png) |
| ![Contact Groups on a selection](2026-10-24/web/menu-contact-groups.png) | ![Title menu](2026-10-24/web-next/menu-title.png) |
| ![Conversation menu](2026-10-24/web/menu-conversation-more.png) | ![Find in conversation](2026-10-24/web-next/thread-find.png) |
| ![Contact Group options](2026-10-24/web/menu-contact-group-options.png) | |
| ![Account menu](2026-10-24/web/menu-account.png) | |

## Screens side by side

Each row pairs the screens that answer the same need. A blank cell means the
app has no such screen.

| web | web-next |
|---|---|
| ![Login](2026-10-24/web/login.png) | ![Login](2026-10-24/web-next/login.png) |
| | ![Create a new account](2026-10-24/web-next/create-account.png) |
| | ![Home](2026-10-24/web-next/home.png) |
| ![Conversations](2026-10-24/web/conversations.png) | ![Group Messages](2026-10-24/web-next/group-messages.png) |
| ![A conversation open](2026-10-24/web/conversation-open.png) | ![A group conversation open](2026-10-24/web-next/group-open.png) |
| ![Contacts](2026-10-24/web/contacts.png) | ![All contacts](2026-10-24/web-next/contacts.png) |
| ![A contact open](2026-10-24/web/contact-open.png) | ![A contact open](2026-10-24/web-next/contact-open.png) |
| | ![A contact's direct thread](2026-10-24/web-next/contact-direct-thread.png) |
| ![A Contact Group](2026-10-24/web/contact-group.png) | ![A label](2026-10-24/web-next/label.png) |
| ![No Contact Group](2026-10-24/web/no-contact-group.png) | ![No label](2026-10-24/web-next/no-label.png) |
| ![Unknown](2026-10-24/web/unknown-contacts.png) | ![No messages](2026-10-24/web-next/no-messages.png) |
| ![A Saved Search](2026-10-24/web/saved-search.png) | |
| ![No Message Tag](2026-10-24/web/no-message-tag.png) | |
| ![Search](2026-10-24/web/search-messages.png) | ![Search](2026-10-24/web-next/search.png) |
| ![Trash](2026-10-24/web/trash.png) | ![Trash](2026-10-24/web-next/trash.png) |
| ![Settings](2026-10-24/web/settings.png) | ![Settings: Account](2026-10-24/web-next/settings-account.png) |
| | ![Settings: Access](2026-10-24/web-next/settings-access.png) |
| | ![Settings: Storage](2026-10-24/web-next/settings-storage.png) |
| | ![Settings: Appearance](2026-10-24/web-next/settings-appearance.png) |
| ![Owner: User Accounts](2026-10-24/web/owner-accounts.png) | |

Web's Settings is one screen with tabs (Account, Profile, Storage, Audit
Trail, System, Appearance), so one screenshot stands for all of them. Import
and Export are desktop-only, and the browser sends both routes back to the
conversation list, so neither appears here.
