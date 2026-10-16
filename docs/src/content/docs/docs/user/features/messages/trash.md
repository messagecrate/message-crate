---
title: Trash
description: What moving a conversation or a contact to Trash does, what restoring does, and what deleting from Trash removes.
---

**Trash** in the sidebar holds the conversations and contacts an account has set aside.
Nothing in Trash is deleted.
A trashed conversation keeps its messages and can still be opened and read.

Trash is also the only way to delete one conversation or one contact permanently.
The item must be in Trash before **Delete** or **Empty Trash** can remove it.

## What moving to Trash does

**Move to trash** is in the header of an open conversation and in the drawer of an open contact.

A trashed conversation leaves the conversation list, and a search no longer looks at it or at its messages.
It is no longer counted among the conversations of the contacts in it.

A trashed contact leaves the Contacts list.
The contact's conversations stay where they are, because trashing a person is separate from trashing what was said.

## A trashed contact and a later import

A trashed contact stays in Trash until an import meets one of the contact's identities.
A backup that still holds the person means the person is still in use, so the import discards the trashed contact and makes a new contact from the backup, as a first import would.
The discarded contact's name, its Contact Group memberships, and every identity it had go with it.

The new contact carries only the identities the backup mentions.
The same phone number on another service counts as mentioned.
An identity the trashed contact had that the backup does not mention belongs to no contact afterwards, and its conversations show under Unknown until the identity is added to a contact.

A contact that should stay out of Contacts for good must be deleted from Trash, because a trashed contact is replaced by the next import that meets it.

## The Trash screen

The Trash screen has two sections, **Conversations** and **Contacts**.

Trashed conversations are listed in the left column.
Selecting one shows its name and message count in the pane, with **Restore**, **Delete**, and **View conversation**, which opens the conversation for reading.

Trashed contacts are listed in the pane itself, because a trashed contact cannot be opened.
Each row has its own **Restore** and **Delete**.
The pane lists up to 100 trashed contacts.

**Restore** takes the item out of Trash, and its row leaves the screen.

The search box narrows both sections at once.
A search word that only one section understands is not applied to the other section, and that section says so, for example `participants: applies to conversations only`.

## Deleting

**Delete** and **Empty Trash** each ask for confirmation first.
The confirmation says what will be removed, because Delete means different things for a conversation and for a contact.

Deleting a conversation removes it and its messages from the Message Crate.
An attachment that only the deleted messages use goes with them.
An attachment that another message also uses stays, because an attachment is stored once however many messages share it.

Deleting a contact removes the name and details, along with the contact's Contact Group memberships.
The contact becomes Unknown.
The messages stay, and the conversations show the phone number or address in place of the name.
Deleting a contact never deletes a conversation.

**Empty Trash** does both at once.
Every conversation in Trash is deleted with its messages, and every contact in Trash becomes Unknown.
It acts on all of Trash, not only on what a search is showing.

Deleting needs the account's delete permission, which the Owner sets.
Without it, **Delete** and **Empty Trash** are disabled and show `Deleting is not permitted for this account`.
Moving to Trash and restoring stay available.

The record of an Import Run under **Settings → Storage → Import history** does not change when conversations the run brought in are later deleted.
It describes the run as it happened.

## Searching for trashed items

Trash is a mark on the item, not a place its data was moved to, so a search can ask about it.
The `trashed:` operator works on contacts, conversations, and messages:

- `trashed:yes` finds only trashed items.
- `trashed:no` finds only items that are not trashed. Every search does this by default.
- `trashed:any` finds both.

[Search](/docs/user/features/messages/search/) covers the other operators.

## Deleting everything at once

**Settings → Account** has a **Danger zone** with **Delete all messages**.
It deletes every message and attachment the account holds without trashing each conversation first, and leaves the contacts and settings in place.
The button is disabled on the `demo` account.

## What deleting erases from the disk

Deleting a conversation or a contact from Trash, **Empty Trash**, **Delete all messages**, deleting an account, and an **Edit** load of an [address book](/docs/user/features/settings/account-and-profile/#address-book) each erase the deleted text from the database's files, not only from the screens.
The server writes zeros over the deleted rows and the space they freed and clears the write-ahead log, the `messagecrate.db-wal` file beside the database.
Each of them except deleting a contact from Trash and an **Edit** load also writes the search index again without the deleted words.
A copy of the database's files made afterwards holds none of the deleted text.

Does deleting take longer on a large Message Crate?

Yes, by up to about a second on a Message Crate of 636,000 messages, because the search index is written again whole each time.
Deleting a contact from Trash and an **Edit** load leave the search index as it is, so they take no longer.

When another request is reading the database at that moment, the write-ahead log can't be cleared at once.
The server tries again every 30 seconds until it is cleared, and stopping the server clears it as well.

The database's files are not the only place the messages are kept.
The attachment files, the copies made before the delete, and the sources the messages came from keep what they held.

A deleted attachment's file is removed, but the disk keeps its bytes in free space until something else is written there.
Full-disk encryption is what keeps them unreadable on a lost or stolen disk, as [Encrypt the disk that holds the volume](/docs/user/features/owner/run-on-another-machine/#encrypt-the-disk-that-holds-the-volume) describes.

A backup of the data directory or an export made before the delete holds the messages it was made with until it is deleted.
So does the copy the server keeps before a schema rebuild, named like `messagecrate.db.before-rebuild-1176793189`, as [Update Message Crate](/docs/user/features/owner/update/) describes.

The phone or app backup the messages were imported from still holds them, and so do the conversation files the desktop app wrote while importing it.
Deleting from Message Crate leaves both as they are.

What about text deleted for good before Message Crate 0.11.0?

Releases before 0.11.0 left its words in the search index and its rows in the database file's free space.
The first delete for good after the update writes the search index again without those words.
The free space keeps the rows until that space is used again.
With the server stopped, the `sqlite3` command-line tool erases both at once.
It must be version 3.43 or newer, because older versions can't merge this kind of search index.
On the desktop app or a server run without Docker, the command runs in the data directory:

```bash title="With the server stopped: write the search index and the database file again"
sqlite3 messagecrate.db "INSERT INTO messages_fts(messages_fts) VALUES('optimize'); INSERT INTO message_versions_fts(message_versions_fts) VALUES('optimize'); VACUUM;"
```

A Docker install keeps the data directory in its volume, so a temporary container with `sqlite3` runs the same command there, with the `message-crate` container stopped:

```bash title="With the message-crate container stopped: the same command in the volume"
docker run --rm -v message-crate-data:/data alpine sh -c \
  "apk add --no-cache sqlite >/dev/null && sqlite3 /data/messagecrate.db \"INSERT INTO messages_fts(messages_fts) VALUES('optimize'); INSERT INTO message_versions_fts(message_versions_fts) VALUES('optimize'); VACUUM;\""
```
