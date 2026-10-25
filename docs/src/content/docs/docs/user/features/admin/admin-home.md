---
title: Admin Home
description: The five sections of Admin Home, and how the administrator adds, changes, disables, and deletes accounts.
---

The administrator, `admin`, logs in to **Admin Home** in place of a message list.
The administrator runs the Message Crate and holds no messages, so Admin Home has no **Messages**, **Contacts**, **Import**, **Export**, or **Trash**.

Nothing in Admin Home shows a message's text, an attachment's contents, or a contact's name.
The administrator sees counts, sizes, dates, and attachment file names.

The side panel lists five sections: **Dashboard**, **Server Settings**, **User Accounts**, **Audit Trail**, and **Logs**.
Login opens **User Accounts**.

## Dashboard

**Dashboard** shows what the Message Crate holds across every account, in three parts.

**Contents** shows the total size of all attachments.
Under it are the number of messages, attachments, conversations, and contacts.

**Database** shows three sizes:

| Figure | Meaning |
|---|---|
| **Database size** | The size of the database on disk. |
| **Messages on disk** | The part of the database the messages take. |
| **Full-text search index** | The part the search index takes. |

**Database size** leaves out attachment files, because they are stored as files beside the database and are counted under **Contents**.
The search index is one structure shared by every account, so it has one figure and no split by account.

**Messages by account** has one row per account, `admin` first.

| Column | Meaning |
|---|---|
| **Account** | The account's username. |
| **Messages** | How many messages the account holds. |
| **Text** | The size of the text of those messages. |
| **Estimated size on disk** | The account's share of **Messages on disk**. |

The estimate is **Messages on disk** split by each account's share of the text.
The last row, **All accounts**, carries the totals.
The row of `admin` reads zero, because the administrator holds no messages.

## Server Settings

**Server Settings** holds the attachment size limit, the space the Staging Directory takes on this computer in the desktop app, the Demo Account, and three facts about the server.

Only the administrator creates accounts, under [User Accounts](#user-accounts).
No setting lets anyone else make one, and the login card has no way to make an account.

### Attachment size limit

**Attachment size limit** is the largest file an import can upload as an attachment, in MB.
It is 512 on a new Message Crate.
The administrator types a new number and selects **Save**.
Any number above zero is accepted.

A new limit holds for every upload after it is saved, with no restart.
An Import Run already under way keeps the limit it started with, so its Staging Review still describes what it will upload.

### Staging Directory on this computer

In the desktop app, **Staging Directory on this computer** says how much disk space the whole Staging Directory takes on the computer running the app.
That is everything in the directory **Settings → System** names.
It includes the Import Runs of every account that imports from this computer, into this Message Crate or another.
It also counts any run still staged under an earlier setting.
The Staging Directory is on that computer, so a browser doesn't show it.
The size is measured when Server Settings opens.
Each account sees what its own Import Runs take under [Settings → System](/docs/user/features/settings/system/#space-used).

### Demo Account

The Demo Account holds made-up conversations.
Anyone who reaches the server opens it with **Explore Demo Account** on the login screen, and it has no password.
On a server [published on the network](/docs/user/features/admin/run-on-another-machine/#only-on-a-trusted-network), that means every device on the network.
Every new Message Crate starts with it.

- **Add Demo Account** shows when there is none. It builds the account with the size chosen beside it.
- **Reset Demo Account** shows when there is one. It removes the account, with everything visitors changed in it, and builds it again. It asks first. No other account is touched.

The sizes are **Medium**, about 54,000 messages, and **Large**, about 613,000 messages.
Medium takes a few seconds. Large takes about a minute.
The card shows **Building the Demo Account** until the build ends, and the server keeps working meanwhile.
Nobody can be in the Demo Account during a build: the build logs out anyone already in it, and the login screen offers **Explore Demo Account** again once the build ends.

The Demo Account is removed under **User Accounts**, as any account is, with **Delete account** in its **Danger zone**.
That is how the administrator closes it to the network, because no setting does.
A restart doesn't bring it back, because the server builds it only for a new Message Crate.
**Add Demo Account** builds it again.
Its page there shows its status, permissions, and identities, and none of them can be changed.

Below the Demo Account:

- **Version** is the version of the server that is running.
- **Schema fingerprint** is a number the server derives from its database layout. [Update Message Crate](/docs/user/features/admin/update/#when-the-database-layout-changes) says what a changed number means.
- **Journal mode** reads `wal` when the server runs the database with write-ahead logging, as it should. Any other mode means the server could not turn it on, and every page waits while an import writes: [Troubleshooting](/docs/user/features/admin/troubleshooting/#admin-home-says-the-server-could-not-turn-on-write-ahead-logging) says what to do. **unknown** means the server could not read the mode, and its log says why.

## User Accounts

**User Accounts** lists every account, `admin` first.

| Column | Meaning |
|---|---|
| **User** | The username, with the account's display name under it when it has one. |
| **Status** | **Administrator**, **Active**, or **Disabled**, with **No password** under it for an account that has no password: it can't log in until the administrator sets one. The Demo Account has no password by design and is not marked. |
| **Last login** | The date and time of the last login, or **Never**. |

The search bar at the top reads **Search accounts**.
It narrows the list to the accounts whose username or display name contains the typed text.

A gear appears at the left of a row when the pointer is over it.
The gear opens that account's settings.

### Add an account

1. Select **Add account**. The **New account** form opens. Its **Profile** and **Storage** tabs can't be opened yet, because they belong to an account that exists.
2. Enter a **Username**.
3. Enter a **Password** and repeat it in **Confirm password**. The heading reads **Password (optional)**, because the form allows an account with no password, but an account with no password can't log in until the administrator sets one. A password is at most 128 characters.
4. Select **Create**.

The form reports **Invalid username.** when the username is already taken.

The screen changes to **User Settings: _username_** for the new account.
The administrator stays logged in.
The new account is **Active**, with **Import**, **Export**, and **Delete** all on.

### User Settings for one account

The gear on a row opens **User Settings: _username_**, with the display name in brackets when the account has one.
**← User Accounts** at the top goes back to the list.

The screen has three tabs: **Account**, **Profile**, and **Storage**.

#### Account

**Username** shows the username. It can't be changed.

Under it, two more parts are for reading only:

- **Last Login** is the date and time of the account's last login, or **Never**.
- **App** is the app the account last connected with, **Desktop app** or **Website**, and that app's version. It reads **Never connected.** for an account that hasn't connected. When the app's version differs from the server's, a line under it gives the server's version. The server serves that app all the same.

**Status** is **Active** or **Disabled**.
The server refuses a disabled account's login.
Disabling the account ends any Session it has open, so the person logs in again after the account is enabled.
The login answers a disabled account the same way it answers a wrong password.
The administrator tells the account's holder that the account is disabled.
The account's messages stay where they are.

**Message Permissions** has three checkboxes: **Import**, **Export**, and **Delete**.
Each one covers messages and their attachments.

**Status** and **Message Permissions** save when they are changed. There is no save button.
The account's holder sees the same status and permissions in their own **Settings** and can't change them.

**Change Password** has **New password** and **Confirm new password**.

- **Change password** sets the new password.
- **Reset password** removes the password, so the account can't log in until the administrator sets a new one.

Neither one ends a Session the account has open, and neither makes the person choose a new password at the next login.
The administrator doesn't see the old password and isn't asked for it.

**Danger zone** is closed until it is selected. It holds two actions:

| Action | Button | What it deletes |
|---|---|---|
| **Delete all messages & attachments** | **Delete all messages** | The account's messages and attachments. The account, its contacts, and its settings remain. |
| **Delete this account** | **Delete account** | The account with all its contacts, messages, and attachments. |

Each button asks for confirmation first, and the confirmation states how many messages will be deleted.
Neither deletion can be undone.

**API Tokens** lists the account's API Tokens, with each token's name, permissions, and when it was created, last used, and expires.
The token itself isn't shown, not even in part.
The trash button on a row revokes that token after a confirmation, and programs using it stop working at once.
The administrator can't add or rename a token, because a token is the account's own credential.

#### Profile

The administrator sets the same three things the account's holder sets:

- **Display Name**. It saves when the administrator leaves the field, presses Enter, or moves to another tab.
- **Time Zone**, the zone the account's message times are shown in. It saves when a zone is chosen.
- **Identities**, the account holder's own phone numbers and email addresses.

#### Storage

**Storage** shows what the account holds, as its holder sees it:

- **Usage**: the size of the account's attachments, with its counts of messages, attachments, conversations, and contacts.
- **Import history**: each import, with its date, type, message count, attachment count, and size.
- **Export history**: each export, with its date, scope, status, and counts.
- **Largest attachments**: file names and sizes.

Two things the holder sees are left out for the administrator, because they say who the account talks to.
The administrator doesn't see which contacts an import created, and doesn't see which conversation a large attachment is in.

### The administrator's own settings

The row of `admin` opens **Settings for Administrator**.
**Settings** in the account menu, the round button at the top right, opens the same settings.

The tabs are **Account**, **Profile**, **Audit Trail**, and **Appearance**.
There is no **Storage** tab, because the administrator holds no messages.

Changing the administrator's password needs **Current password** as well as the new one, because the administrator's login reaches every account.
There is no **Reset password**, because the administrator must have a password.
There is no **Danger zone**, because `admin` can't be deleted.
A lost administrator password is set again on the server, as [Troubleshooting](/docs/user/features/admin/troubleshooting/#the-administrators-password-is-lost) describes.

## Audit Trail

**Audit Trail** lists what each user did on the Message Crate, and when, newest first: logins and how each Session ended, refused logins, Import Runs and Export Runs, and every change to an account.
The **Account** column names the account each entry is about.
**Account** at the top narrows the list to one account, the entries its holder reads in the **Audit Trail** tab of their [Settings](/docs/user/features/settings/audit-trail/).

Nobody can change or delete an entry, the administrator included.
A deleted account's entries stay under its old username, marked **deleted**.
**Account** lists deleted accounts under **Deleted accounts**, below the live ones, each by its old username and when it was deleted.
Picking one narrows the list to the entries and runs that account left.
Another account given the same username, before or after, keeps its own entries apart.
Each change to the attachment size limit is recorded too, under no account, with the new limit.
Adding the Demo Account is recorded under no account, and resetting it under the Demo Account it replaced, each with the set of Demo Data asked for.

**View in log** on an entry opens [Logs](#logs) on the server's log with **Show** at **Everything** and the id of the request that made the entry in **Search the log**, so only that request's lines show.
If the server's log holds no line of the request, Logs says so: either its oldest files were deleted as it grew, or `RUST_LOG` leaves out its `INFO` lines, as `RUST_LOG=warn` does.
An entry made outside a request, such as one a server command made, an Import Run, an Export Run, or a Session that ran out, has no **View in log**.

## Logs

**Logs** shows a log, newest line first.
**Log** at the top picks which one:

- **The server's log**, what the server did for every account. It opens on this one.
- In the desktop app, **Desktop app**: what the desktop app went on without outside an Import Run, such as a program it downloaded whose record it could not write. It is listed once the app has written a line.
- In the desktop app, each Import Run's log on this computer, whoever ran the import, under **Import Runs on this computer**. Each is named by its run, the account that ran it, and when its last line was written.

A browser offers the server's log only, because the desktop app's log and an Import Run's log stay on the computer that wrote them.

Each line shows its time in UTC, its level, and what it says.

- **Show** picks the levels: **Errors**, **Warnings and up**, or **Everything**. It opens at **Warnings and up**.
- **Search the log** keeps the lines that contain the typed text, ignoring case, as it is typed.
- Scrolling to the end reads older lines.
- **Download** saves the log as it is. The server's log is up to five files, and each has its own button.

The list shows the log as it was when it was read, and does not add lines as the server or a run writes them.
A log names counts, routes, files, and what went wrong, and never what a message says.
Where each log is kept and how long is covered in [Where the app keeps its files](/docs/user/features/settings/system/#where-the-app-keeps-its-files).
