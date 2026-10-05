# Changelog

What changed in Message Crate, written for the people who use it.

Each release is grouped three ways:

- **Features** — something you can now do that you could not before.
- **Fixes** — something that was wrong and now behaves correctly.
- **Design** — a change in how the product works that is worth knowing about,
  including work under the surface that changes nothing on screen.

Internal rework is mentioned only as far as it matters to you. The reasoning
behind a decision lives in the architecture notes under `docs/adr/`, and the
detail of any single change lives in its pull request.

Version numbers follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Bullets under the version still in development carry the date they landed;
released versions carry their date on the heading.

## [0.10.0] — in development

### Features

- 2026-10-04 **An edited Apple Messages message keeps every earlier
  version, and search finds it by any of them.** A message edited in Apple
  Messages is imported with its final text and each version before it, with
  the time each one was written. Searching Messages for a word that only an
  earlier version held finds the message. A later import leaves a message
  already stored as it is, with its text and earlier versions. Export keeps the
  earlier versions, so a conversation exported and imported again keeps them
  too. The conversation shows them, and which one a search found, in a
  coming release.
- 2026-10-04 **The conversation and the Messages list show which messages
  were deleted in the app they came from, and which were unsent.** A message
  deleted in the source app keeps its text in a faded bubble with a dashed
  outline, and the line under it reads like "4:56 PM · Deleted in Apple
  Messages". An unsent message is an empty faded bubble with a dashed
  outline that reads "Unsent", with its time under it. Every source draws
  them the same way. The Messages list marks them too: a row for a message
  deleted in the source app keeps its text, with a faded "Deleted in Apple
  Messages" line under it, and a row for an unsent message reads "Unsent" in
  place of its text. The Demo Account has a few of each in its Apple
  Messages conversations.
- 2026-10-04 **A message deleted in Apple Messages, or unsent, is kept and
  marked.** A message deleted in Apple Messages that its recently deleted
  list still holds is imported with its text and marked Deleted in the
  source app. A message its sender unsent is imported as Unsent, rather than
  as a line saying someone unsent a message. A message only partly unsent
  keeps what is left and has no mark. Search finds marked messages like any
  other, and `deleted:yes` or `deleted:no` on Messages narrows to them or
  away from them. Nothing is hidden. A later import of the same message
  that carries the mark adds it to the message already there. Export keeps
  the mark, so a conversation exported and imported again keeps it too.
- 2026-10-04 **A long conversation scrolls without downloading its photos,
  and videos and voice notes play in place.** A photo or video shows as a
  small thumbnail, loaded only when its message scrolls near the screen,
  with its file name in its place until the server has made it. A video
  shows a play button and loads nothing until it is pressed; then it plays
  in the conversation and seeking loads only the part sought to, where
  before a video downloaded whole before its first frame. A voice note has a
  play button of its own. The viewer keeps the thumbnail up while the full
  photo loads and has the next and previous photos ready. A HEIC photo, a
  HEVC video or an AMR voice note opens as the copy every browser can show,
  the same in every browser, and says so when that copy is not made yet.
  Every attachment has a download button, which always saves the original
  under its own name; the desktop app asks where to save it.
- 2026-10-04 **Photos and videos get their browser copies after every
  import, without anyone asking.** Once an import ends, the server makes a
  small thumbnail of every photo and video it brought, and a copy every
  browser can show of each HEIC photo, HEVC video, voice note and other file
  browsers often cannot play. It works in the background, so the import
  finishes as soon as its messages are in, and a server stopped part-way
  finishes the rest when it starts again. Until now those copies existed
  only after someone ran a command on the server.
- 2026-10-04 **The server keeps its log in files the owner can read.** Docker
  and the desktop app's server now write their log to a `logs` directory
  beside the database, as well as to their output, so the lines that explain a
  failure are still there after a restart. It keeps at most 250 MB, in five
  files of 50 MB, and deletes the oldest file when a new one starts. The owner
  can read it through the server's interface, newest first, narrow it to
  errors or warnings, search it, and download a whole file. No other account
  can. A Logs screen on Owner Home that shows it comes next. The log never
  holds a password, a token, a search, message text, an attachment or a contact's
  name, phone number or email address.
- 2026-10-04 **A search word the other list takes stays in the box.**
  Switching between Conversations and Messages with a word only one of them
  takes, such as `from:me` on Conversations, no longer shows an error. The
  word is underlined with a red wavy line and the list searches with the rest.
  Click the word to see which list it works in, and **Remove** it there if you
  no longer want it. Switching back searches with it again.
- 2026-10-04 **Notes you sent to yourself are their own conversation.** A chat
  with your own number or email, such as Apple Messages to yourself or
  WhatsApp's "Message yourself", imports with no one else in it and no
  longer adds you to Contacts. It goes by your display name, or by the
  address when you have none, and changes when your display name does.
  Search `with:me` to list exactly those conversations.
- 2026-10-04 **The Audit Trail narrows to a deleted account.** Owner Home's
  Account picker lists deleted accounts below the live ones, each by its
  old username and when it was deleted. Picking one shows only what
  that account did and what was done to it. Another account given the same
  username, before or after, keeps its own entries apart.
- 2026-10-03 **An Audit Trail of what each user did, and when.** Owner Home's
  Activity panel is now the Audit Trail: every login, session ending and
  refused login, every import and export, and every change to an account,
  newest first, with who did it and from which app. The owner reads every
  account's and can narrow it to one. Each person reads what concerns their
  own account under Settings, including what the owner changed. Nobody can
  edit or remove an entry, and an account's entries stay, under its old
  username, after the account is deleted.
- 2026-09-22 **One identity table, on the contact drawer and on an account's
  Profile.** An account's identities now show what a contact's do: the
  service, the address, when it was first and last heard from, and how many
  conversations, direct messages and group messages it takes part in. The
  columns line up under their headers, the sort arrow sits next to the
  label, and every row ends with a visible Remove. Adding an identity opens
  a small dialog instead of a permanent row under the table, and the dialog
  offers Email everywhere, so a contact can be given an email address by
  hand.
- 2026-09-22 **The Dashboard shows where a Message Crate's disk space goes.**
  Owner Home's Dashboard is now three sections. Contents is the card it
  had. Database shows the size of the database on disk, how much of it the
  messages take and how much the full-text search index adds, all measured
  by the server. Messages by account lists every account with its message
  count, its text and an estimated size on disk, split from the messages
  figure by each account's share of text, with a totals row so the split
  visibly adds up. Attachment files are counted under Contents, not in
  the database size.
- 2026-09-24 **A WhatsApp import knows which number is yours.** Every
  imported WhatsApp message now records the phone number your WhatsApp
  account is registered to, so its conversations count toward that identity
  in Settings. An iPhone backup carries the number, and Import reads it from
  there. An Android backup does not, so the Import form asks for it in a
  **WhatsApp phone number** field, pre-filled from your profile's phone; on
  iPhone the same field sits under Processing Options as a fallback for a
  backup without the number. The number is recorded on the messages and is
  not added to your profile.
- 2026-09-24 **Search contacts by what they sent you.** On Contacts, every
  word that counts or dates messages now counts only the messages the contact
  sent you, in a direct or a group conversation. `messages:0` is everyone who
  never messaged you, which is what the Advanced Search form's Never messaged
  always said; `date:2019` is everyone who wrote to you in 2019; and
  `first-message:` and `last-message:` are the first and last message a
  contact sent, so `-last-message:>=2022` lists everyone you have not heard
  from since 2022, including people who never messaged you. Your own messages
  and other people's messages in a shared group conversation no longer count
  towards a contact. On Conversations and Messages the same words still mean
  the conversation's messages. The contact drawer's message count follows the
  same rule. The Advanced Search contacts form's date fields are First message
  and Last message, and Trash's form has them too.
- 2026-10-01 **The desktop app is a Message Crate on its own.** The
  installer now carries the server and the website. When you open the app
  and nothing answers on this computer at its usual address, the app starts
  its own Message Crate, keeps its data in your system's app-data directory,
  and stops it when the app closes. A Message Crate already running there,
  such as one in Docker, is used as it is. Settings → System has an **Open
  data directory** button. Trying Message Crate no longer needs Docker.
- 2026-10-01 **Every new Message Crate starts with the Demo Account.** A
  Message Crate with no database creates one holding the Demo Account and
  its Demo Data, about 54,000 messages, before it opens its doors, whether
  Docker or the desktop app started it. The login card has an **Explore Demo
  Account** button, so nobody has to look up a username. No owner is made
  for you any more, so a Message Crate never has a published owner password:
  you claim it by creating the Owner. The Demo Account's limits are fixed
  rather than settings: it can export, it can't import or delete messages
  for good, and nobody can change its password, status, permissions or
  identities. Its photos, videos and audio show and play on a computer with
  no ffmpeg.
- 2026-10-01 **The Owner adds or resets the Demo Account from Owner Home.**
  Server Settings has a Demo Account card that adds it back after it was
  deleted, or puts it back the way it started, with a choice of the medium
  set or the large one of about 613,000 messages. No restart and no command
  line are needed. The Demo Account's page under User Accounts is read-only
  apart from Delete.
- 2026-10-01 **The Owner sets the attachment size limit.** The largest
  attachment the server accepts is now one number under Owner Home → Server
  Settings, 512 MiB until the Owner changes it. The desktop app reads it
  before Staging, so the Staging Review, Media and the Upload measure every
  file against the limit that is really in force. Before, the desktop app
  left out any file over 50 MiB of its own accord, whatever the server would
  take.
- 2026-10-01 **A conversation shows an attachment's Preview.** A HEIC photo
  or an HEVC video imported with Attachments → Copy now shows in any
  browser, once the server has made its Preview. Opening the attachment
  still gives the original.
- 2026-10-01 **The Address Book is a spreadsheet you export, edit, and load
  back.** Contacts arrive with your messages, so the Address Book is no
  longer where they come from. It is how you fix many of them at once.
  **Export** on the Contacts screen writes the contacts you are looking at
  (a search, a Contact Group such as Unknown, or the rows you checked) to a
  CSV file with one row for each identity. Fill in names, Contact Groups,
  and identities in a spreadsheet, then load the file under Settings, on the
  Profile tab. **Append** adds and renames and removes nothing. **Edit**
  also makes each contact in the file match its rows, so deleting a row
  takes that identity off the contact. Contacts the file does not mention
  are left alone. A load does exactly what the file says and never corrects
  it: a name in the file replaces the contact's name, even one you typed in
  the app, and a row moves only the identity it lists, so to move a number
  on both Text Message and WhatsApp, give each its own row. A file with a
  mistake in it is refused whole, and each row at fault is listed with its
  reason, so nothing is half loaded. Message Crate no longer reads a phone's
  vCard file, which put every number on a card into your contacts whether
  or not a message ever used it.
- 2026-10-01 **The Demo Account has Contact Groups.** Demo Data is now built
  the way your own Message Crate is: its messages are imported first, and an
  Address Book then names the people in them and puts them in Family, Work,
  College, and Inactive.
- 2026-10-01 **The header names the account you are logged in as.** The
  username sits beside the account button for every account, the Owner
  included, so on a Message Crate several people share you can see whose
  messages are on screen.
- 2026-10-01 **Import and Export say when your account may not use them.**
  In the desktop app, an account the Owner has not allowed to import or
  export sees a message saying so in place of the form, instead of filling
  it in and being refused partway through. The Demo Account's Import screen
  says importing needs a personal account and offers **Log out**.
- 2026-10-02 **WhatsApp imports from an encrypted iPhone backup.** WhatsApp
  → iPhone on the Import form now has an **Encryption password** field, the
  same one iMessage has. The app sees that a backup is encrypted and asks
  for the password before the import starts. The encrypted backup you made
  for your iPhone messages now serves for WhatsApp too; a second,
  unencrypted backup is no longer needed.
- 2026-10-03 **Export and Convert write SMS Backup+ mail.** Choose **EML
  (SMS Backup+)** to get your SMS and MMS back as the mail SMS Backup+
  writes, one directory per conversation, which the SMS Backup+ import reads
  again and any mail program can keep. Other messages are left out, and the
  log says how many.

### Design

- 2026-10-04 **Every answer from the server carries every field.** A field
  with no value now arrives as `null` instead of being left out, so a program
  written against the server's reference finds the same fields in every
  answer of one kind, and the reference marks each of them as always there.
  Nothing changes on screen.
- 2026-10-04 **A reaction travels on the message it reacts to.** An Apple
  Messages tapback or emoji reaction is written into an export on the
  message it reacts to, with the person who reacted named, and an import
  stores it under that person. Export writes the reactions the server keeps
  the same way, so a conversation exported and imported again keeps them.
  Every app's reactions will take this one shape; Apple Messages is the first,
  and WhatsApp follows in a coming release.
- 2026-10-04 **The server sends an attachment a piece at a time.** A video
  or voice note can start playing before the whole file has arrived, and a
  player can jump to any point in it without downloading what comes before.
  The server also hands the app a short-lived link to one attachment, which
  works for an hour and ends when you log out, so a player built into the
  page can load it. The app's players start using both in a coming release;
  nothing on screen changes yet. The architecture notes record how
  attachments are to be shown: a small thumbnail in the conversation, the
  original or a browser-ready copy chosen by file type when one is opened,
  and the original whenever one is downloaded.
- 2026-09-22 **An account identity means ownership.** The Profile tab now
  says what the identities are for: your phone numbers and emails, which
  Import uses to determine which messages belong to you. The glossary and
  the architecture notes record the same distinction: a contact's identity
  means the person took part, an account's means the messages are theirs.
- 2026-09-22 **Profile Setup shows the identities already on your account
  in their own fields.** Phone numbers and emails the Owner added
  now fill the rows, where you can change or remove them before going on,
  instead of sitting in a line of text above them.
- 2026-09-22 **Shorter wording on two screens.** The screen that creates
  the Owner now opens with "An owner is required to create and manage
  users.", and
  the Display Name button in Settings reads Save without changing to Saved.
- 2026-09-23 **A contact's identities read the same as an account's.** The
  contact drawer now shows each identity in the form Message Crate stores it,
  a phone number in international form, and names an email address as
  Email, just as the Profile tab does. Message Crate counts both tables the
  same way. Under the surface, the server's interface and code were renamed
  to use the words the product uses, with nothing else to see.
- 2026-09-30 **The project moved.** The repository is now
  `messagecrate/message-crate`, the documentation is at
  <https://messagecrate.app/docs/>, the hosted product answers at
  <https://my.messagecrate.app>, and the Docker image is
  `bitrealm/message-crate`. Every error response's `type` URL now points at
  the new documentation host.
- 2026-09-30 **Message Vault is now Message Crate.** Every screen, every page
  of the documentation, every error message and the HTTP API reference use
  the new name, and the word "vault" is gone from all of them. One
  installation is "a Message Crate", the account that runs it is the
  "Owner", and the owner's installation-wide settings are "Server Settings".
  The desktop app's window and installers carry the new name.
- 2026-09-30 **Everything that was named after the old product has a new
  name, and nothing old still works.** The owner's routes are under
  `/v1/server`. Session and API tokens start `mc-user-` and `mc-api-`, so
  every existing token stops working and everyone logs in again. The
  database file is `data/messagecrate.db` and several tables are renamed, so
  an existing database is rebuilt empty and needs a fresh import. The
  Staging Directory defaults to `~/message-crate`. The compose service is
  `server`, and the desktop app installs as a new application beside any
  older copy.
- 2026-10-01 **The user guide starts with the desktop app.** It is now in
  two parts. Try Message Crate installs the desktop app and looks around the
  Demo Account. Your own messages creates the Owner and an account, backs up
  a phone, and imports it, all on the same Message Crate. Docker is no
  longer the first step.
- 2026-10-01 **The server runs on SQLite only.** A Message Crate could also
  keep its database on a Postgres server, which existed for a hosted service
  that is not built yet. That option is removed, so the server is simpler to
  run and to change. Postgres support comes back with the hosted service.
  The last of the server code written to choose between the two engines is
  gone too, with nothing to see.
- 2026-10-01 **Force reprocessing is gone from the Import form.** It changed
  nothing on a new Import Run, and on a resumed Upload it only sent
  everything again, which made the resume slower and the Duplicate counts
  higher.
- 2026-10-01 **The server refuses a configuration file it doesn't
  understand.** A section or key the server does not use, a misspelt one
  included, stops it at startup with the name and section of each, instead
  of being ignored.
- 2026-10-02 **A stopped Upload is paused, not finished.** Pressing the
  Upload's button now pauses the Import Run and keeps what it staged, and
  the next visit to Import offers to resume it, sending only the
  conversations not yet sent. Before, the run was recorded as completed, the
  staged files were deleted, and the conversations it had not reached were
  never imported. The run's report now puts every conversation in exactly
  one count, and names the ones the stop left unsent. An Upload that fails,
  or sends some conversations and fails the rest, is paused the same way
  instead of being recorded as failed or finished, and the Import badge
  reads Paused. Logging out during an Upload asks first, then pauses the
  Upload before the session ends, so the same account can resume it after
  logging in; before, every conversation left was recorded as failed. When
  the server doesn't record a run as finished, the staged files are kept and
  the next visit resumes the run, which then gets its Saved Search and
  Contact Group. A resumed run's report covers every part of the run, not
  only the last. A Staging or Media Stage that fails deletes its staged
  files at once, since nothing can resume it, instead of leaving a full
  unencrypted copy of your messages in the Staging Directory.
- 2026-10-02 **One desktop job runs at a time.** An export won't start while
  an Import Run's job runs, and the reverse, and the screen says why. Before,
  the two jobs mixed up each other's progress, and one Cancel stopped both.
- 2026-10-02 **Every phone number in Demo Data is one reserved for
  fiction.** No Demo Data number can be dialled or belong to a real person.
  Demo Data messages also fall in the daytime, between 08:00 and 23:00,
  where most of them used to fall overnight.
- 2026-10-03 **Convert reads JSON Lines files only when they end in
  `.jsonl`,** the name Message Crate gives them. A file ending in `.ndjson`
  is no longer taken for one.

### Fixes

#### Importing

- 2026-10-04 **An iMazing import keeps each of two pictures sent in one
  second, and every picture stays with its own message.** When two photos
  with one file name arrived in the same second, iMazing saved them as
  `image0.jpg` and `image0 2.jpg`, and the import kept only the first
  message, because it compared the file name the rows gave and not the
  files. It now compares the files: two different pictures are two
  messages, each with its own picture, and two copies of one picture are
  still one message. Dropping that second message also moved every later
  picture of the import onto the message before its own. Each picture now
  goes to its own message.
- 2026-10-04 **An iMazing import reads a phone number or an email address
  the same way everywhere.** A sender, a chat name and a name in a group's
  member list are now read by the one rule every other import uses. A chat
  named `tel:` and a number is that number's conversation, and a group
  member listed that way is that number, not a second member by that name.
  An email address is matched whatever its capitals. A three-digit service
  number is an address. A sender named rather than numbered, such as
  `AMAZON` or `Promo2024`, is kept as that sender. Before, `Promo2024` was
  read as the number `2024`, and in a chat with a number such a message was
  shown as sent by that number.
- 2026-10-04 **An import lists every backup file it could not read, and
  notes what it kept with a caveat.** A CSV, XML, mail or MMS file in the
  backup that the importer could not read, which before showed only in the
  import log, is now an error in the finished run that names the file.
  Something the import did with an item that is worth knowing but is not a
  failure is now a note in a **Notes** list of its own under the errors:
  an iMazing Live Photo video that two rows claim, an SMS Backup+ message
  that lost a part it could not read, a message that records no
  phone number for the other person, a group message that names none of
  your numbers, a group member kept by an email address, a chat kept under a
  name alone, and a WhatsApp attachment whose file is not in the backup,
  each named. The notes are kept with the run, so Storage shows them later
  too, and a run with notes and no errors still reads as completed.
- 2026-10-04 **Staging's progress no longer jumps forward when a file the
  backup names is not there.** The byte total counted such a file's size
  and took it off only when Staging reached it, so the percentage leapt
  ahead partway through. A file that is not on disk, and an attachment
  with no bytes, are now left out of the total before Staging starts.
  Apple Messages from an encrypted iPhone backup is the one exception:
  its files are inside the backup, so one that is missing there is still
  found only when Staging reaches it.
- 2026-10-04 **Resuming an import waits while another job runs.** The
  Import screen offered to resume a paused or waiting import while an
  export or a conversion was running, and the desktop app then refused
  it. The resume button now stays off until that job ends and says which
  job it is waiting for, as the Import form does.
- 2026-10-04 **An import whose app closes or crashes keeps every Error
  found so far.** Staging reported its Errors only when it finished, Media
  reported none, and an Upload's skipped attachments waited for the end of
  the Upload, so an app that closed partway lost them. Every Error now
  reaches the run the moment it is found. A file Media could not convert,
  or left out because it was still too large after converting, is listed
  as an Error with the file and the reason; one it could not convert keeps
  its original, and its Error goes when a resumed Media converts it. A
  resumed run lists an Error it finds again once. Discarding a run after
  a crash during a resumed Upload no longer records a conversation as
  failed when that Upload had already sent it.
- 2026-10-04 **The server's `import` command names the file it refuses an
  attachment in.** When an attachment broke a rule, such as a path that
  leaves the export directory or bytes that do not match the fingerprint
  the file states, the `import` command stopped with the line and the rule
  but not the file. Every refusal now prints the same way: the file, then
  the line and the rule.
- 2026-10-04 **A group text from an SMS Backup+ archive stays one
  conversation when a member's contact gained an email address.** SMS
  Backup+ names a person by their email address when their contact on the
  phone had one at backup time, and by their number otherwise, so one group
  could split in two. The import now learns each address's number from that
  person's own texts in the archive and keys the group member by the number.
  A member the archive never gives a number for keeps their email address,
  and so does one whose address it gives two numbers, as a contact card
  two people share does. The run's summary counts both.
- 2026-10-04 **A received SMS Backup+ group text that doesn't name you shows
  its sender's name.** Such a text is filed under its sender, who showed up
  as a bare address. The sender now gets the name the mail gives them.
- 2026-10-04 **Logging out during an Upload waits at most 15 seconds, and
  an Upload whose session ends pauses cleanly.** Logging out during an
  Upload waited for the Upload to pause for as long as that took, and an
  Upload that did not stop kept you logged in. Logout now waits at most 15
  seconds, with a **Log out now** button, and then logs out anyway and
  says the Upload resumes from what it had sent. When your session ended
  while an Upload ran and nothing else noticed, the Upload kept going and
  recorded every remaining conversation as failed. It now pauses at once,
  records none of them as failed, and logs you out.
- 2026-10-04 **A group imported from two backups takes its current name,
  however the backups arrive.** When two copies of one group chat became
  one conversation, the name it ended with depended on whether they came
  in one upload or two: one upload kept the first copy's name, and two
  uploads kept the second's. The conversation now takes the name of the
  copy whose messages run later, which is the name the group has now. An
  older backup uploaded afterwards no longer brings back a name the group
  has dropped, and a copy with no name never clears one.
- 2026-10-04 **Working files from reading a backup no longer stay behind
  when an import is stopped, and Staging checks for room while it reads
  the backup.** Reading an encrypted iPhone backup decrypts its message
  database, and reading an SMS Backup & Restore, GO SMS Pro or SMS Backup+
  file sets its attachments aside, both as plain copies. These working
  files used to sit in the Staging Directory, and an app closed or stopped
  mid-run left them there until a later run used the same directory. They now
  sit in the app's cache directory, are deleted when the run ends, whether it
  finished or failed, and are deleted when the app next starts if it was
  stopped. Before it writes, Staging now checks that the disk holding the
  Staging Directory and the disk holding the cache directory have room, and
  stops with the space it needs while it reads the backup, not after.
- 2026-10-04 **A discarded import keeps its errors, and an import keeps its
  converted files when they cannot be read back.** Discarding an import,
  or cancelling it at a Review, recorded it with no errors, though it had
  some. It now keeps the errors it had recorded, including the
  conversations a paused Upload could not send. When Media finished but
  its files could not be read back afterwards, the import ended as failed
  and deleted the converted files. It now goes back to the form, and
  resuming it reads the files again.
- 2026-10-04 An internal fix to how an upload finishes an Import Run;
  nothing you see changes.
- 2026-10-04 **An iMazing import no longer reports a Live Photo choice as an
  error, and leaves WhatsApp chat directories' extra files alone.** When two
  photo rows named one picture, the import gave its Live Photo video to the
  first of them, as it should, but its report listed that as an error. The
  report now lists it as a note. The import also looked for Live Photo
  videos and link previews in WhatsApp chat directories, though only iMazing's
  Messages export holds them. It could attach a video beside a WhatsApp
  photo to that photo's message. It also counted the directory's other files
  as left out. It now looks for them in Messages chat directories only.
- 2026-10-04 **iMazing and Apple Messages imports name a directory they cannot
  read in full.** When an iMazing chat directory held an entry that could not
  be read, the rows whose files were in it came through with no photo or
  file, and a Live Photo video in it was dropped, without a word. On a Mac,
  a Contacts account directory that could not be read left that account's
  names out of an Apple Messages import with no message, and one Contacts
  account whose database could not be read left out every account's
  names. The iMazing import now stops and names the directory. The Apple
  Messages import now names each Contacts account it cannot read in its
  log and keeps the names from the others.
- 2026-10-04 **Minimum Video File Size takes a number of megabytes.**
  Compress & Convert added an `M` to whatever was typed in Minimum Video
  File Size, so `20MB` became `20MBM` and the run was refused over a value
  nobody typed. The field now takes a whole number of megabytes, such as
  `20`, as its label says. Anything else is refused before Staging with a
  sentence that names what was typed and asks for a number of megabytes.
  A cleared field is refused too, where before it quietly became 20.
- 2026-10-04 **A resumed Import Run follows the attachment setting Staging
  recorded.** An Import Run resumed after the app closed decided from its
  saved form whether it had a Media Stage, and its Upload took the size
  limit per file from the saved form too, not from what Staging recorded
  when it prepared the files. Nothing kept the two in agreement. A resumed
  run now reads both from what Staging recorded, so it converts or
  compresses exactly what Staging prepared for, and Upload holds each file
  to the limit the Staging Review showed.
- 2026-10-04 **An Import Run's progress no longer steps back or stops
  short during Staging.** While the Staging Stage copied attachments, two
  conversations finishing at the same moment could send their counts out of
  order or mixed together, so the attachment and conversation counts could
  move backwards and end on 3 of 4 when every one had been staged. Each
  count now follows the one before it, and both end on the full total.
- 2026-10-04 **An attachment too large after conversion says so when an
  interrupted Media Stage resumes.** When two conversations shared one
  attachment whose converted copy came out over the size limit, an Import
  Run resumed after the app closed during its Media Stage could record the
  attachment as missing in one conversation instead of too large. It now
  records it as too large, with the converted size.
- 2026-10-04 **An Apple Messages import stops when the Apple Messages
  reader stops.** The Apple Messages reader (imessage-reader) could stop
  while an encrypted iPhone backup's attachments were being copied. Every
  attachment still to come was then recorded missing, with one log line
  each. The Import Run ended looking like a run with many missing
  attachments. It now stops at that point with an error that says the
  reader stopped and why. No attachment is recorded missing because of it,
  so the import can be run again once the cause is gone.
- 2026-10-04 **A person known only by name keeps a conversation of their
  own.** SMS Backup+, iMazing and OpenExtract backups sometimes name a person
  without recording a number or address. That person's conversation could
  merge with another's: two names in a script other than Latin, such as
  "张伟" and "李娜", became one conversation, and so did "Ana Lee" and
  "Ana.Lee". A person named "AMAZON" shared the conversation of the sender
  AMAZON, and a person named "unknown" shared the conversation of sent
  messages that name nobody. Each person now keeps a conversation of their
  own. Such a person also became two contacts on import, one holding their
  name and one with no name; they are now one contact.
- 2026-10-03 **A video, photo or audio file that cannot be converted says
  why, briefly.** The error for a file the Media Stage of an Import Run
  could not convert held the converter's version, build settings and progress
  lines, with the reason at the end of several kilobytes. It now holds only
  the lines that say why the conversion failed.
- 2026-10-03 **An attachment with a blank file name is refused.** An
  import that gave an attachment a blank file name, empty or only spaces,
  stored that blank name when the server already held the file. The import
  is now refused and names the line of the file that holds it, as it
  already was when the server did not hold the file.
- 2026-10-03 **Staging's progress no longer jumps forward.** When a backup
  listed photos or files it did not hold, their sizes counted toward the
  total and came off only as Staging reached each one, so the percentage
  jumped. The total now leaves them out from the start.
- 2026-10-03 **An Upload that cannot read its staged files pauses and names
  the directory.** When the app could not read part of the files an Import Run
  had staged, the conversations it could not see were left out without a
  word, and the Upload reported success. The Upload now pauses and names
  the directory, so it can be resumed once that directory can be read.
- 2026-10-03 **A message sent twice a moment apart is shown twice.** When
  one source held a message sent twice a second or two apart, and another
  source held one copy, matching the sources hid all but one of the three.
  Now only the extra copy is hidden.
- 2026-10-03 **A photo or file imported from two places is stored once, not
  once for each.** The same picture from an Apple Messages backup and from
  an SMS Backup & Restore file used to be saved twice. It is now saved once
  for your account, and it stays as long as a message from either place
  still has it.
- 2026-10-03 **You are no longer listed among the people in your own group
  conversations.** When a backup named one of your own phone numbers or email
  addresses among a group's members, an import added you to the group as
  another person, with a contact of your own. Any address on your account now
  stays out, on every service, even one the backup did not know was yours.
- 2026-10-03 **Group texts from an SMS Backup+ archive are group
  conversations.** Every group text used to be filed as a conversation with
  one of its members alone. A group text you sent and the replies to it now
  land together in one group conversation, each reply credited to the person
  who sent it. A group text whose sender matches nobody in the group shows
  no sender instead of the first member. Each such group text is also listed
  among the import's issues.
- 2026-09-23 **A group text from an SMS Backup & Restore backup is no longer
  credited to the wrong person when the backup names no sender.** A group
  MMS without a sender address was shown as sent by whichever member the
  backup happened to list first. Such a message now shows no sender, as a
  message with no recorded sender does from any other source. A message that
  does name its sender was already credited correctly, whichever position the
  sender holds in the group.
- 2026-09-23 **An iMazing message sent in the hour the clocks spring forward
  is kept.** iMazing writes each message's date as a wall-clock time with no
  zone. A time that never showed on the clock, such as 02:30 on the March
  morning when 02:00 became 03:00, was dropped as an invalid date; it is
  now read with the offset in force just before the change, so it lands at
  the instant the new clock called 03:30. A time that showed twice on the
  November morning the clocks fall back is the earlier of the two. The
  zone an iMazing export is read in can now be given by name, such as
  `America/New_York`, as well as by offset.
- 2026-09-24 **GO SMS Pro picture messages import whole, and the ones you sent
  import at all.** An import read only the picture messages you received and,
  for most of them, mistook bytes inside the picture for phone numbers, so a
  photo from one friend could land in a group conversation with hundreds of
  made-up members. Every picture message now lands with the people who were
  actually on it, the ones you sent are included, and a voicemail notice from
  Google Voice stays in the Google Voice conversation instead of being moved
  under the caller.
- 2026-09-24 **iMazing message times are read in your account's time zone.**
  An iMazing export writes each message time without a zone, and the desktop
  app used to read them in whatever zone the computer running the import was
  set to, so the same directory gave different times on different machines. The
  import now reads them in the time zone on your profile. If the phone lived
  in another zone at the time, Processing Options on the Import screen has a
  Time zone of the messages picker for the iMazing source.
- 2026-09-24 **An iMessage you sent is always yours, however the phone
  recorded your number.** Some iPhone databases store the sending number on
  an outgoing message as `tel:+1…`, and the import kept that prefix, so those
  messages carried a sender that did not match the number on your profile.
  The prefix is now removed the same way for every message and for the list
  of addresses the backup sent from.
- 2026-10-01 **Resuming an Import Run asks again for the backup password or
  WhatsApp key.** A resumed run read the backup with an empty password and
  failed, with nowhere to type it.
- 2026-10-01 **The Import form marks every field it needs.** With **SMS
  Backup+**, **Backup Device Email Addresses** had no asterisk, and with
  **iMazing** and **OpenExtract**, **Backup path** had none, though the
  Import button stays disabled until each is filled. The Android SMS form's
  **Backup Directory** and **Backup Device Phone Numbers** lacked one too.
  All now carry the asterisk, and **Backup Directory** describes the chosen
  source's own backup files.
- 2026-10-01 **Max FPS is a ceiling.** Compress & Convert re-encoded every
  video at exactly Max FPS, so a 24 fps video came out at 30. A video at or
  below the limit now keeps its frame rate.
- 2026-10-01 **The Saved Search and Contact Group an Import Run adds work.**
  Opening the Saved Search an import added showed an error instead of that
  run's messages. Two runs from one source on the same day shared one
  Contact Group, and an import could take over a Contact Group you had made
  with the same name. Each run now gets a Contact Group of its own, and a
  Saved Search you create while an import finishes no longer costs the run
  its own.
- 2026-10-02 **One attachment no longer stops a whole import.** An
  attachment whose file name the computer refuses, such as one with a
  240-character extension, ended the Staging and nothing was imported. It
  is now recorded as missing and everything else is staged. A run with media
  turned off is no longer refused for lack of disk space it would never
  use, and a staged attachment whose name ends `.jsonl` no longer turns a
  successful Staging into a failed run.
- 2026-10-02 **Large SMS backups no longer run out of memory.** The SMS
  Backup & Restore, GO SMS Pro and SMS Backup+ readers kept every attachment
  in memory until the end of the run, so a backup of several gigabytes of
  video could get the desktop app stopped by the operating system. They now
  hold one attachment at a time.
- 2026-10-02 **An Upload interrupted at the wrong moment no longer stores
  messages twice.** When the connection dropped while the server's answer to
  a batch was arriving, the batch was sent again and messages without their
  own id were stored twice. Every message now carries an id, so a batch
  sent again for any reason stores nothing twice. A file holding a message
  without one is refused whole, naming the first lines at fault.
- 2026-10-02 **Empty attachments and lowered size limits no longer fail a
  conversation.** A 0-byte attachment was refused, and the whole conversation
  that held it failed. Lowering the attachment size limit during an Upload
  broke an attachment already uploading in parts. The limit also capped
  every request, not only attachments, so a very low limit stopped anyone
  logging in or raising it again; it now holds attachments alone. A small
  video in Compress mode is forecast at its own size, so the Staging Review
  warns when it won't fit.
- 2026-10-02 **An Import Run stops when the server can't record its stage.**
  A lost stage write left the run behind, so the next visit offered only
  Start over, or read the whole backup again, instead of showing the Review.
- 2026-10-02 **Cancel pressed just before a job starts stops it.** A Cancel
  in the moment between two steps of an import or an export was lost, and
  the next step ran to the end.
- 2026-10-02 **Two conversations with one person in one batch become one.**
  Two conversations whose addresses are the same once written the same way,
  such as `+15555550119` and `5555550119`, or an iMessage and an SMS
  conversation with one number, failed the whole batch when they arrived
  together. They now merge, as they did when they arrived apart.
- 2026-10-02 **Messages sent in the same second keep their order.** When an
  Upload split a conversation inside one second, the conversation and every
  export showed those messages out of order.
- 2026-10-02 **A message sent twice is shown twice.** When two sources each
  held a message sent twice in one second, matching the sources against
  each other hid all but one copy.
- 2026-10-02 **An import no longer matches a name to a contact in the
  Trash.** A participant the backup names without an address was bound to a
  trashed contact of that name, which failed the import or made a third
  contact beside a live one.
- 2026-10-02 **Upload errors name the real cause.** An attachment check the
  server refused said "username does not match API key" or "invalid API
  key", causes that no longer exist. It now says the account is disabled or
  may neither import nor export, or that the login was not accepted.
- 2026-10-02 **Apple Messages addresses arrive as themselves.** A phone number
  and an email address on one contact card arrived as a single address made of
  both, stored as an email identity. A received message with no sender became
  a contact called "Me". A conversation with no members lost its name, an
  unnamed group with one member left was filed as one-to-one, and your own
  address was listed among a group's participants. Each is fixed.
- 2026-10-02 **No decrypted copy of your iPhone messages is left behind.**
  Entering an encrypted backup's password made a plain copy of the Messages
  and Contacts databases in the system's temporary directory, which stayed
  there when the app or the reader was stopped. The copy now lives in the
  run's own directory and is always removed. Decrypted attachments are written
  there too, on the disk the run checks for room, so a small system
  temporary directory no longer makes every large video fail. An attachment
  that could not be decrypted is counted and listed among the Import Run's
  issues with the reason, instead of being marked missing while the run
  reported success.
- 2026-10-02 **WhatsApp imports keep what they could not find.** A message
  whose photo file was missing lost the attachment with no trace, and a later
  import with the file in place added the message a second time. It now
  keeps the attachment marked missing. WhatsApp Status no longer
  becomes a contact, and WhatsApp ids that are not phone numbers are no
  longer stored as email identities. A WhatsApp import from an iPhone backup
  no longer fails over leftovers from an earlier run in the backup directory.
- 2026-10-02 **Phone numbers from Android SMS backups keep their country.**
  The SMS Backup & Restore, GO SMS Pro and SMS Backup+ readers read every
  number by US rules, so `+6595550100` became a US number and a UK number
  matched nobody. An email address became a phone number made of its
  digits, and a message from a sender name such as `AMAZON` was dropped. Your
  own number written without its country code is now recognised as yours,
  so a received picture message is no longer filed as a group with you in
  it.
- 2026-10-02 **SMS Backup & Restore messages read back as written.** Line
  breaks in a message survive, an emoji written as a character reference
  shows as the emoji, and a broken reference costs one character instead of
  the message, or, in GO SMS Pro, the whole file. The names "null" and
  "(Unknown)" no longer name a contact, and the sender of a group picture
  message is no longer named after the whole group.
- 2026-10-02 **GO SMS Pro picture messages with newer headers import.** A
  picture message using a header from a later version of the MMS standard was
  dropped whole.
- 2026-10-02 **SMS Backup+ imports only text messages, and reads every
  directory you give it.** A call-log mail is now skipped and counted, instead
  of imported as a text holding the call's length. A backup directory that sits
  inside a directory named Duplicate, Exclude or `.git` is no longer skipped
  whole. Two people known only by names that differ outside plain English
  letters, such as "张伟" and "李娜", no longer share one conversation.
- 2026-10-02 **iMazing imports attach Live Photo videos.** A Live Photo's
  video is imported with its picture, and link previews and any other file
  no row names are counted in the report. A message no longer picks up
  another message's file whose name merely ends the same way, and a group
  known only by names no longer gets its own id as a member.
- 2026-10-02 **An attachment too large after conversion says so in every
  conversation.** When two conversations shared one attachment that came
  out of Media over the size limit, the second recorded it as missing
  instead of too large. Opening the link to an Import Run's directory not made yet
  says nothing is there yet, instead of calling it outside the Staging
  Directory when that directory is reached through a link.
- 2026-10-02 **An Apple Messages reaction belongs to the person who made
  it.** Your heart on a friend's message was stored as theirs, and theirs on
  yours as yours. Each reaction also became an extra message of its own, and
  removing a reaction showed it as added. A reaction is now stored on the
  message it reacts to, under the person who made it, and a removed one
  leaves nothing behind.
- 2026-10-02 **Two different messages alike are no longer stored as one.**
  For SMS Backup & Restore, GO SMS Pro, SMS Backup+, iMazing and
  OpenExtract, "lol" from two people in the same second of a group, or "?"
  sent twice a moment apart, was stored once and the other copy counted as
  a duplicate. Reading the same backup on a computer set to another time
  zone gave every message a new id, so importing it again stored every
  message a second time. Each message's id now takes in its sender and the
  exact instant, wherever the backup is read, and every source drops a
  repeated copy of one message in the same way.
- 2026-10-02 **The Import form's attachment choices are checked before
  Staging and hold for the whole run.** Compress & Convert with Max FPS
  cleared ran Staging to the end, hours on a large backup, then failed, and
  every resume failed the same way. It is now refused before Staging starts,
  with a sentence that names Max FPS; a Max FPS of 0 or below is refused
  too. An iMazing or OpenExtract import always copies the original
  attachments. Before, it followed whatever Attachments choice was left from
  another source, so it could re-encode videos or show "Attachments: Skip".
- 2026-10-02 **An import refused over a line that can't be read names a line
  you can find.** The server's `import` command counted only the lines that
  were not blank, so the number pointed at the wrong line. An Upload named a
  line of its own batch; it now names the staged file and the line in it.
- 2026-10-04 **An SMS Backup & Restore import counts the repeated messages
  it drops.** A message the backup held twice was kept once, as it should
  be, but the import's summary never said a copy was dropped. It now says
  how many, as it does for GO SMS Pro, SMS Backup+, iMazing and OpenExtract.
- 2026-10-04 The way a resumed Upload records the conversations an earlier
  part of the Import Run already sent was reworked, with nothing visible.
- 2026-10-04 Handling for messages without an id, which an import already
  refuses, was removed, with nothing visible.

#### Exporting and converting

- 2026-10-04 **Exporting to CSV, and reading an EML or mbox file back,
  type a phone number written with `tel:` as a phone number.** The
  `identity_type` column of a CSV export, and a participant read back from
  an EML or mbox file, now follow the one rule every import uses. Before,
  `tel:+15555550157` was typed `other` there and `phone` everywhere else,
  so the same person could arrive as two identities.
- 2026-10-04 **Converting no longer asks for room for an attachment whose
  file is missing.** Convert checks the disk for room before it starts,
  and that check counted the size an attachment's record gave even when
  the file was not in the export being converted, so a conversion could
  be refused for space it would never use. An attachment with no file is
  now left out of the check, and a refused conversion still leaves the
  earlier output as it was.
- 2026-10-04 **Exporting Apple Messages to EML or MBOX with attachments
  embedded no longer asks for room for a file that is gone.** From a Mac,
  or from an iPhone backup that is not encrypted, the check for room
  counted an attachment whose file was missing at the size Messages
  recorded. It now counts nothing for it, and the log names the file. In
  an encrypted iPhone backup the files are inside the backup, so each one
  is still counted at its recorded size.
- 2026-10-04 **Converting, and exporting Apple Messages to a format other
  than JSON Lines, no longer count an attachment with no file in the byte
  total.** The byte total in the progress and in Convert's log counted
  the size such an attachment's record gave and took it off only when the
  run reached it, so the total dropped partway through. Every attachment
  known to have no file is now left out of it before the run starts, as
  Staging already does. The Apple Messages check for room for the
  attachment files it writes leaves it out too. Convert's log also names
  each file it found missing, as Staging's does.
- 2026-10-04 How an exporter writing a format other than JSON Lines counts
  the size of an attachment with no file was reworked to match Staging,
  with nothing visible.
- 2026-10-04 **Converting an SMS Backup & Restore backup says what it
  left out.** The log said nothing about the repeated copies it dropped,
  the messages with an invalid date, a date outside the range, no usable
  address or an unknown type, the drafts, the picture messages with nobody
  on them, the message parts it could not read, or the character codes
  that stand for no character, and it named
  only the first five files it could not read. It now lists each count
  as soon as the backup is read, even when the conversion then stops, and
  names every file it could not read.
- 2026-10-04 **Exporting from a second server or account no longer makes
  the first download every attachment again.** When Export from two
  servers, or two accounts, wrote into one directory, the run that
  finished last forgot which attachments the other had already
  downloaded, so the other's next Export downloaded all of them again. Each
  server and account now keeps its own record.
- 2026-10-04 **Nothing an export did not write is ever removed.** Every
  step that removes or replaces files from an earlier export, including
  the obfuscated export's placeholders, now checks for itself that an
  export wrote the directory, and refuses one that it did not. Before,
  three of those steps relied on the step before them to check.
- 2026-10-04 **Messages sent to nobody survive an export as SMS Backup+
  mail.** OpenExtract keeps sent texts that name no recipient in one
  conversation. Exported as **EML (SMS Backup+)** and imported again, they
  came back as a conversation with a made-up person, who was then added
  to Contacts. They now come back as the same conversation with no one in
  it.
- 2026-10-04 **A person known only by a name that looks like a number keeps
  their messages through an SMS Backup+ export.** A person a backup named
  "+1 555 0101", with no address, lost every message when exported as
  **EML (SMS Backup+)** and imported again. The import now keeps them in
  that person's conversation.
- 2026-10-04 **Every format checks for room before it writes.** Only an
  import to the server used to check for free disk space; writing CSV,
  JSON, EML, MBOX or SMS Backup & Restore XML, and **Convert** in
  Settings, failed part-way with a write error when the disk filled. They
  now check first and stop with the space they need. **Convert** from an
  SMS Backup & Restore backup also sets the backup's attachments aside in
  the app's cache directory, not in the directory it writes to.
- 2026-10-04 **A conversion can no longer start in the middle of an Import
  Run or an export.** **Convert** in Settings stayed disabled only while one
  of the run's Stages was running, so it could be started while an Import
  Run waited at a Review, or between an export reading the messages and
  writing them in the chosen format. The run's next part was then refused
  because another job was running. **Convert** now stays disabled from the
  start of an Import Run or an export to its end, and **Export** waits
  through an Import Run's Reviews too. Both are enabled again as soon as the
  run finishes, fails, is paused, or is cancelled or discarded. A Review holds
  them back only for the account that started the run, so another account
  logged in on the same desktop app can still export and convert; the
  Review's approve button then waits for that job to end.
- 2026-10-04 **A group text exported to SMS Backup & Restore names the
  people in it, not one sender.** An export to SMS Backup & Restore put the
  name of whoever sent each group message where the app keeps the names of
  everyone in the group, so the file held a name the app would not write
  there, and reading it back lost that name anyway. Each group message now
  carries the names of the people in the group, as SMS Backup & Restore
  writes them. One-to-one messages are unchanged.
- 2026-10-04 **An export stops when it cannot read an earlier export's
  email directory.** An export or conversion written into a directory an earlier
  one used first removes the earlier one's email conversation directories. A
  conversation directory holding an entry that could not be read could look
  as if it held no email, and then stayed beside the new export. The
  export now stops and names the directory.
- 2026-10-04 **An obfuscated export keeps a reply to a message in another
  conversation.** Apple Messages can reply or react to a message in another
  conversation. In an obfuscated export, that reply or reaction pointed at
  no message. It now points at its message, wherever in the export that
  message is. A message the export leaves out, such as one outside the date
  range, still can't be pointed at.
- 2026-10-04 **GO SMS Pro and SMS Backup+ attachments keep their size when
  their files are left out.** A run with Attachments set to Skip wrote each
  GO SMS Pro and SMS Backup+ attachment without its size. Each one now
  carries its size, as SMS Backup & Restore attachments already did.
- 2026-10-04 **An obfuscated export no longer records the real size of each
  photo or file.** Obfuscate replaces every attachment with a placeholder,
  but the attachment kept the size of the real file, which can be enough
  to recognise it. An obfuscated attachment now carries no size.
- 2026-10-04 **Convert keeps the previous output when an Android XML backup
  can't be read.** Converting a broken `smses.xml` into a directory an earlier
  conversion wrote removed that conversion's files before the backup was
  read, so the run failed with nothing left. The backup is now read first,
  as every other input is, and a backup Convert can't read, or one with no
  conversations in it, stops the run with the previous output left as it
  was.
- 2026-10-03 **An obfuscated export leaves out attachments in subdirectories
  too.** A real photo or file inside a subdirectory of the export's
  attachments stayed in the export that exists to leave it out. A shortcut
  (symbolic link) there is now removed, and the file or directory it points
  to is left untouched.
- 2026-10-03 **Android XML holds only SMS and MMS.** Export and Convert
  wrote every message as a text message, so an iMessage or a WhatsApp
  message came back from a re-import as an SMS. They now leave every other
  message out, and the log says how many were left out and why.
- 2026-10-01 **An export from the Conversations list holds those
  conversations.** Export opened from a filtered Conversations list wrote
  only the matching messages, or refused a search such as `messages:>100`.
  It now writes every message of the conversations the list showed.
- 2026-10-02 **Converting to CSV, EML or MBOX and back loses nothing the
  server reads.** Each message keeps whose number it was sent from, an
  attachment keeps its size and the reason it is missing, and a text or HTML
  attachment comes back with its own bytes instead of the next attachment's.
- 2026-10-02 **An export stays inside its directory.** An attachment is never
  written outside the output directory, whatever path it was stored under. An
  export, or Convert, whose output directory is the backup or a directory above it
  is refused before anything is written. An obfuscated export that can't
  remove a real attachment fails and names the file, instead of reporting
  success with the file still there.
- 2026-10-02 **Exported attachments are checked.** A download that answers
  with something other than the attachment, such as a sign-in page from a
  proxy, is refused, instead of being saved as the attachment and skipped
  by every later export into the same directory.
- 2026-10-02 **Export and Convert name the directory they wrote to.** The
  success message followed whatever the form showed afterwards, and the
  directory could be changed while the job ran.
- 2026-10-02 **Convert names a file from an older format.** A file in the
  version-3 format is refused by name, where a directory of them failed with
  a message that said nothing useful and a directory mixing them with current
  files left those conversations out without a word. A refused file now
  stops the run before the previous output is removed.

#### Search

- 2026-10-04 **A conversation opened from a Message Tag page keeps the
  search box as it was.** Opening a conversation from a Message Tag page put
  `tag:Holiday` into the search box, as though it had been typed, and the
  Messages list then showed every message with that tag instead of asking
  for a search. The tag now stays out of the box: the box shows only what
  was typed, and the lists are the same in the conversation as on the tag
  page.
- 2026-10-04 **Searching for part of a group conversation's id no longer
  lists every group conversation.** Each source gives its group
  conversations ids of one shape, such as `group:…`, `chat-…` or `…@g.us`.
  So typing `group`, `chat` or `g.us` on Conversations listed every group
  conversation from that source, whatever its title. `in:`, `with:` and
  `identity:` found them the same way. A group conversation is now found by
  its title and by the people in it.
- 2026-10-04 **Searching Conversations for `name` no longer lists every
  conversation known only by a name.** Typing `name` on Conversations listed
  every conversation whose backup gave a name and no address, and `less` the
  conversation that names nobody; `in:nam` on Messages listed every message in
  them. Each is now found by the name of the person in it or by its title,
  with plain text on Conversations and with `in:` on Messages.
- 2026-10-04 **`identity:` no longer lists every conversation known only
  by a name.** `identity:nam` listed every conversation whose backup gave a
  name and no address, and `identity:less` the conversation that names
  nobody. Neither search lists them now, unless someone in them has an
  identity that matches.
- 2026-10-04 **Import, Export and Settings show no search box.** The search
  at the top searches the list of the section you are in, and these screens
  have no list yet, so the box there searched nothing. On Export, typing in it
  changed which conversations Export would export. The box is gone
  from these three screens and comes back when you return to a list.
- 2026-10-03 **A search pasted and run at once is the search that runs.**
  Pasting a search and pressing Enter straight away searched for nothing.
  Typing very fast lost letters, and the search ran on the last letter
  typed. The box now keeps everything you put in it, and Enter runs it.
- 2026-09-23 **Excluding something from a search no longer hides the rows
  that have nothing to compare.** A search with `-` in front of a word left
  out every row with no value for that word, so those rows appeared under
  neither the word nor its negation. `-import:last` found no messages at all
  before the first import, and a negated date word on Contacts left out
  every contact with no messages. A search and its negation now always
  divide the list between them.
- 2026-09-23 **Searching for a word with punctuation in it works.** A
  search such as `a&b`, `o'bri*`, or text pasted with a hidden NUL
  character failed with an error. Punctuation inside a word now always
  means the words next to each other in that order, and a NUL is read as
  a space.
- 2026-09-24 **The Trash stays out of a search everywhere the search looks.**
  A contact whose only group conversation was in the Trash still matched
  `kind:group`, a conversation whose only Family member was in the Trash still
  matched `group:Family`, and `conversations:` counted trashed conversations.
  A search now leaves the Trash out on both sides until it uses `trashed:`,
  and then the Trash counts on both sides, which is what searching the Trash
  screen already did. The contact list's Last heard from date and its ordering
  leave trashed conversations out too, so they agree with `last-message:`.
- 2026-10-01 **Search forgets a deleted message's attachment name.** After
  a message was deleted, a search for its attachment's file name found the
  next message imported.
- 2026-10-02 **The search box sends the search you typed.** Picking a
  suggestion after a `-` dropped the minus and reversed the search. A Contact
  Group or Message Tag whose name holds a comma, a leading `#` or a trailing
  `*` is quoted. "Between" with only an end date includes that day. Text
  typed on Trash or a tag page stays inside that page, so `or` no longer
  brings in conversations from outside it. A second Enter runs the text the
  box shows.
- 2026-10-02 **Searches that failed now answer.** A date the account's time
  zone skipped, such as 30 December 2011 in Samoa, dropped the connection. A
  date beyond year 9999 compared the wrong way round. A long comma list
  failed with a server error instead of being refused as too complex. An
  empty quoted phrase, `""`, matched everything; it is now refused.
- 2026-10-02 **Search finds Greek and Turkish names.** `name:ΚΩΣ*` now
  finds "ΚΩΣΤΑΣ", and `name:istanbul` finds "İstanbul Office".
- 2026-10-02 **An attachment added to a stored message is searchable.** An
  import that added a missing attachment to a message already in Message
  Crate left its name out of search.
- 2026-10-02 **`service:` on Contacts reads the whole conversation.** A
  contact you texted over SMS who never replied is now found by
  `service:sms`.
- 2026-10-02 **Each account sees only its own recent searches.** The next
  account to log in on the same browser was offered the last one's.
- 2026-10-02 **Searches and filters stay put.** Opening a conversation from a
  searched or filtered list kept the list filtered only until the first
  click. A Saved Search click no longer rewrites the Contacts or Trash search
  you go Back to. Trash names an unknown search word once, and says when
  its requests fail instead of saying "Trash is empty."
- 2026-10-02 **`source:` names every source an import reads.** It took only
  `imessage`, `whatsapp` and `sms`, where `sms` meant SMS Backup & Restore,
  so messages from iMazing, OpenExtract, GO SMS Pro and SMS Backup+ could not
  be searched by source. It now takes `imessage`, `whatsapp`,
  `sms-backup-restore`, `imazing`, `openextract`, `go-sms-pro` and
  `sms-backup-plus`, and the Advanced Search form on Messages has a Source
  field listing them by name. On Conversations, `source:` also finds a
  conversation that source holds only as duplicates.
- 2026-10-02 **A conversation of only duplicates shows no date.** Under an
  `import:` search, such a conversation read as last active on 1 January
  1970.

#### Contacts and identities

- 2026-10-04 **An import names a nameless contact, whatever made it.** A
  contact with a number and no name stayed Unknown after an import that knew
  the number's name, when an Address Book load or you had made it rather
  than an earlier import. An import now fills in any contact's missing name.
  It still never changes a name a contact already has.
- 2026-10-04 **A misspelt service no longer puts an identity on Text
  Message.** Adding, swapping or removing a contact's identity, or one of your
  own, takes Text Message or WhatsApp and nothing else. Any other service used
  to be read as Text Message without a word, so a WhatsApp number with a typo
  in its service landed on Text Message; now the server refuses it and says
  which two it takes. An email address is added on Text Message, where
  iMessage reaches it, as the app already does for you.
- 2026-10-04 **One number is one identity however it arrives.** A number
  written with `tel:` in front, in a backup that gave no type for it, became
  a separate identity from the same number as a message sender, on a contact
  of its own. An email address added to a contact under iMessage, or to
  your own identities under Phone, was saved as a phone number. Every
  address is now typed by what it is, whatever service it came over or was
  added under.
- 2026-10-04 **Changing a contact's identity can move it to another
  service.** Changing a WhatsApp number to a Text Message number in one edit
  was refused with "previous address not found on contact". The old
  identity is now found on its own service. Changing a WhatsApp number to an
  email address without naming a service saved an email address on
  WhatsApp; it is now refused with the reason, since WhatsApp carries no
  email addresses.
- 2026-10-04 **An Address Book loaded straight back renames nobody.** A
  name cell that started with a tab, written `'` then a tab in the
  spreadsheet, created a contact whose name kept the tab, and loading the
  exported file back renamed that contact without it and counted it as
  updated. A contact's name is now saved without spaces, tabs or line breaks
  at its start or end, so the file loads back with nothing changed.
- 2026-10-04 **The Contact Groups and Message Tags menus say why a name is
  refused.** Creating a Contact Group from the Contact Groups menu on the
  contacts list, or a Message Tag from the Message Tags menu on the
  conversation list, did nothing when the server refused the name, such as a
  Contact Group name holding `;` or a name over 80 characters. The menu now
  keeps the typed name and shows the reason, as the sidebar already did.
- 2026-10-04 **A screen reader says which contact is open.** The open
  contact in the Contacts list was shown only by its highlight, so a screen
  reader gave no sign of which one was open. The open contact is now
  announced as the current one, in the browser and in the desktop app, and
  one click still opens a contact. In the desktop app the highlight also
  moves to a newly opened or checked contact, where before it could stay
  where it was first drawn.
- 2026-10-03 **Adding a WhatsApp identity checks that it was added.** When
  a number was already a Text Message identity, adding it on WhatsApp
  closed the dialog even if the server added nothing. The dialog now stays
  open and says "The server did not add that identity." When the list of
  identities cannot be loaded again to check an add or a removal, the
  dialog says so and asks you to try again.
- 2026-09-22 **International phone numbers keep their country.** A number
  written with a country code, such as `+65 9555 0100` in an address book or
  `+44 7700 900123` as your own number, is now matched as that number. Before,
  some were read as a US number with the same digits and named the wrong
  person, and some matched nobody.
- 2026-09-24 **A contact's identities and selected contacts count what the
  contact sent.** The identity table on the contact drawer, and the summary
  shown when you select contacts, counted every message in the contact's
  conversations, your own replies and everyone else in a group conversation
  included. So a friend who sent 40 of the 100 messages in your conversation
  showed 100 there and 40 on the drawer's message count. First heard from,
  last heard from, direct messages and group messages now count only the
  messages the contact sent, the same way a search and the drawer's message
  count do, and a conversation in the Trash is left out. The conversation
  count still counts every conversation the contact is in. Your own identities
  on Profile still count every message sent from or received at them.
- 2026-10-01 **Unknown and No group list the right contacts.** Once the
  whole Contacts list had loaded, Unknown listed every contact and No group
  included Unknown ones.
- 2026-10-01 **The Demo Account's email is one of its identities.** My
  Identities showed the email until the list loaded, then dropped it.
- 2026-10-02 **Contact Group and Message Tag names stay usable.** "Unknown"
  and "none" can no longer be taken by a Contact Group, nor "none" by a
  Message Tag, and a Contact Group name can't hold `;`, which the Address
  Book separates names with. A link to a Contact Group or Message Tag opens
  that one, whatever characters its name holds, and a link to one that no
  longer exists says so instead of listing everything. The contact tables
  use the same column names.
- 2026-10-02 **The Address Book load reads a file the way you meant it.** A
  number whose `+` a spreadsheet dropped keeps the identity the contact
  already has, instead of becoming a new US number. A row with a stray extra
  comma refuses the load instead of shifting every cell after it. A refusal
  over a contact in the Trash says to restore it or delete it for good.
  Export writes a `'` before any cell that starts with `=`, `+`, `-` or `@`,
  so a spreadsheet shows a contact named `=HYPERLINK(…)` as text instead of
  running it as a formula, and keeps the `+` of every phone number. The load
  takes that `'` off again, whether or not the spreadsheet kept it.
- 2026-10-02 **A conversation no longer names a contact in the Trash.** A
  participant whose contact was trashed showed that contact's name and
  linked to a contact that could not be opened. A group with no recorded
  members no longer lists its own id as a member.
- 2026-10-02 **Identities count and date the way the rest of the app does.**
  A conversation two identities of one contact share counts once in the
  contact's total. First and last dates are days in your account's time
  zone, not UTC.
- 2026-10-02 **WhatsApp identities stay WhatsApp.** Removing a WhatsApp
  identity in Settings removed the Text message identity with the same
  number, and the WhatsApp one could not be removed at all. Changing a
  WhatsApp contact's number moved it onto Text message. Profile Setup showed
  a number on both services as two Text message rows and would not continue.
- 2026-10-02 **A contact that fails to load says so.** The contact drawer
  and the selected-contacts figures offer Try again, instead of "Loading…"
  for good or a row of dashes.
- 2026-10-02 **A number's messages stay with its contact, whatever service
  carried them.** A message from a contact's number over a service Message
  Crate doesn't know, such as an iPhone message sent by satellite, was filed
  under a new contact with no name and left out of the named contact's
  counts.
- 2026-10-03 **The contact drawer opened from a conversation covers the
  column resize handles.** It sat below the handles, so a handle could show
  through it.
- 2026-10-04 **The contact drawer opened from a conversation stays inside
  the window.** In a window narrower than the list plus the drawer, its right
  side ran past the window's edge and was cut off. It now stays inside the
  window, and in a window as narrow as a phone it covers the list instead.
- 2026-10-04 **The contact drawer opened from a conversation lines up with a
  list that appears after it.** When the list showed up only once the drawer
  was already open, the drawer stayed against the right edge of the window,
  as if there were no list, until the window was resized. It now moves to the
  list's edge as soon as the list appears, and back when the list goes.
- 2026-10-04 **The Contact Identity card and the selected-contacts card fit
  inside their panel.** The Contact Identity card, and the card that sums up
  the contacts you selected, were a little wider than the panel they sit in,
  so the panel scrolled sideways.

#### Accounts, Settings and screens

- 2026-10-04 **On the light theme, the contact drawer's shadow falls on the
  list it covers.** The drawer opens from the right, and its shadow fell to
  the right, under the drawer itself, so the drawer's left edge had no
  shadow. It now falls to the left, as on the dark theme and as the Sources
  drawer's does.
- 2026-10-04 **Panels, menus and drawers stand out on the dark theme.**
  Their shadows were tuned for the light theme and all but vanished on the
  dark theme's dark surfaces. The dark theme now has its own, darker
  shadows, and the drawer that slides in from the right has a thin light
  edge.
- 2026-10-04 **Settings fits a phone-width window.** In a window as narrow as
  a phone, Settings scrolled sideways, because its row of tabs was wider than
  the page and the navigation panel kept its full width. The tabs now wrap
  onto more lines, the navigation panel takes at most half the window, and the
  System and Appearance settings and the Storage page buttons stack to fit.
  Wider windows look as before.
- 2026-10-04 **A focused button shows no white line in the dark theme.**
  The keyboard focus ring on buttons, the contact drawer's close button,
  the date field's calendar buttons and the import form's section headings
  had a thin white line between the button and the ring. The gap now shows
  the colour behind the button, in every theme.
- 2026-10-04 **The focus ring in an import's results and the contact
  drawer sits 1 pixel from the edge.** The sections of an import's results
  and the conversation counts in the contact drawer drew their keyboard
  focus ring 2 pixels out, further than most buttons draw theirs.
- 2026-10-04 **Every button and tab draws its focus ring 1 pixel from its
  edge.** The search box's clear button and its Clear all, the sort button,
  the account menu, the phone numbers in a phone field, the theme choices in
  Appearance, the tabs in Settings and on the login card, the buttons above a
  conversation, the Import history dates and the expand buttons in an
  import's errors and notes drew their keyboard focus ring flush against the
  edge, unlike every other button. They now leave the same 1-pixel gap. The
  day the date picker's keyboard cursor is on shows a 2-pixel ring inside it,
  the same as a focused table row, where it showed a 1-pixel one.
- 2026-10-04 **A Settings tab that can't be opened yet shows the not-allowed
  pointer.** While the owner adds an account, Profile, Storage and Audit
  Trail are greyed out until the account exists, but the pointer over them
  was the plain arrow. It is now the not-allowed pointer every other control
  that is turned off shows.
- 2026-10-04 **The panel resize grips move by exactly 8 pixels.** Each
  arrow key on the grip of the left panel or the list column moved the
  panel 9 pixels wider or 7 narrower, and pressing the grip without moving
  it widened the panel by 1 pixel. Arrow keys now move the panel 8 pixels,
  or 24 with Shift. Pressing the grip leaves the width as it was. A screen
  reader hears the panel's own width.
- 2026-10-04 **Opening an Import Run in Settings → Storage → Import
  history keeps the list inside the page.** Opening any Import Run made the
  Import history table about a million pixels wide. Every column but Date
  sat far off to the side. Opening one that recorded errors or skipped
  items made the table keep getting wider while it stayed open, and its
  Import Errors and Notes tables showed only their first column, so no
  error or note could be read. The run's details now open below its row at
  the table's own width, and its errors, skipped items and notes show every
  column and scroll inside their own box.
- 2026-10-04 **Import history in Settings → Storage loads quickly however
  many problems your imports recorded.** The list used to bring every error
  and skipped item of every import on the page, so a few large WhatsApp
  imports with thousands of skipped files each could make it slow to open.
  It now shows how many each import recorded, in a new Issues column, and
  reads the problems themselves only when you open that import.
- 2026-10-04 **Deleting your account in the desktop app deletes its
  Staging Directories on this computer.** Deleting your own account during
  or after an import left that import's Staging Directory on disk, with
  nothing to offer it again. The delete dialog now names the account's
  Staging Directories on this computer, deleting the account deletes them,
  and one that cannot be deleted is named afterwards so you can remove it
  by hand.
- 2026-10-03 **An expired session says to log in again.** When your
  session had expired, or was ended from another window, an Upload or an
  Export said "invalid API key", though the app sends no API key. It now
  says the server did not accept the session, and to log in again.
- 2026-10-03 **Two accounts can each have the same email address.** When a
  second account added an email address that another account already had,
  the address was linked as its identity but left off its profile. Each
  account now lists every email address it holds.
- 2026-09-22 **Changing a password checks things in a sensible order and
  says so in full sentences.** Message Crate now checks the current password
  first, then that the new password was typed the same way twice, then that
  it differs from the current one, and tells you only the first thing that
  went wrong. The messages read as sentences ("Current password is
  incorrect.") and the Change password button no longer sits tight against
  the last field.
- 2026-10-01 **Attachments show for an account without Export.** An account
  whose Export permission was off saw no photos, videos or audio in its own
  conversations.
- 2026-10-01 **A refused save says why.** Renaming a contact, and creating,
  editing or deleting a Saved Search, now show the server's reason and keep
  the form open. Deleting a Message Tag or Contact Group asks first.
- 2026-10-01 **Profile Setup and Copy work over plain HTTP.** In a browser
  reaching a Message Crate on another machine without HTTPS, Profile Setup
  stopped with an error, and Copy buttons did nothing.
- 2026-10-02 **Settings → Storage shows every run as it is.** Import and
  Export history page through every run, not only the newest 40. A cancelled
  run reads as cancelled, not failed, and "Cancelled" is spelt one way on
  every screen and in the user guide. A run that has not finished no longer
  shows its start time as its finish. The Import badge goes as soon as a
  waiting run is discarded.
- 2026-10-02 **The API Tokens section says what a token can do.** It
  promised that a token could delete messages, which no token can, and now
  shows when each token expires. The secret of a new token stays on screen
  until you close its dialog, through a stray click, Escape, or leaving
  Settings before it arrives. A token shows only the permissions its account
  holds: one made by an account that may not import no longer says it can,
  and a permission the Owner turns off shows as off on every token. A token
  made while its account lacked a permission keeps it off after the Owner
  turns it on, so make a new token then.
- 2026-10-02 **Delete account asks for what it needs.** An account with no
  password confirms with its username alone, the dialog says username, a
  refusal such as a wrong password shows inside the dialog, and the typed
  password is cleared when it closes.
- 2026-10-02 **Permissions and passwords hold.** An account the Owner does
  not allow to delete can no longer delete itself, and with it every
  message; Settings says to ask the Owner. Password guesses count per
  account however the username is capitalised, and wrong current passwords
  on a password change or account deletion count too. The username `demo`
  stays the Demo Account's after it is deleted, so it can always be added
  back. A deleted account's number is never given to a new one.
- 2026-10-02 **A username counts characters, not bytes.** A 70-letter
  Cyrillic username is accepted.
- 2026-10-02 **Every change shows on every screen.** After a password change,
  a rename, deleting messages, or emptying the Trash, other screens showed
  the old state for up to 30 seconds or until the window was focused. A
  password change now also says the account's API Tokens were revoked.
- 2026-10-02 **An ended session goes to the login screen.** After a session
  expired or ended in another tab, every screen showed an error. A profile
  that fails to load now says so with a retry, instead of opening screens
  the account should not see.
- 2026-10-02 **Lists show every row once, and one click opens it.** A list
  loaded page by page no longer repeats or skips a row when something
  changes between pages. In the desktop app, one click opens a contact in a
  search result or a list sorted by Last heard, the range shows at once, and a
  first page that fits the window still loads the next.
- 2026-10-02 **An action stays with its conversation or contact.** A banner,
  an error or a pending Move to trash on one conversation or contact no
  longer shows on, or closes, the next one you open.
- 2026-10-02 **Settings fields keep what you typed and show what is in
  use.** A display name typed but not saved survives a time zone or identity
  change. The time zone field stores the zone you picked, not another one
  with the same rules today. A Staging Directory that can't be used says why,
  and the attachment size limit no longer offers to save a rounded value.
  Emptying the display name and saving clears it, on your own account and
  when the Owner clears another's; before, the old name came back.
- 2026-10-02 **The connection screen keeps track of where it is.** Applying
  an address that does not answer says so and says whether you are still
  connected. In the desktop app, "Use the Message Crate on this computer"
  starts the app's own one.
- 2026-10-02 **The desktop app follows its Message Crate.** The app's own
  server listens where "Let other devices on this network connect" says,
  and changing the box no longer restarts it during an import or starts a
  second Message Crate while the app uses another one. When a Message Crate
  the app found stops, the app notices and starts its own. A slow Message
  Crate is no longer mistaken for another program on the port.
- 2026-10-02 **The website opens with browser storage blocked.** It stayed
  blank; it now opens with the default theme.
- 2026-10-02 **Small fixes in conversations and dialogs.** A video with no
  stored file shows a file chip instead of nothing. The Sources panel shows
  each share beside the count it measures. A Contact Group or Message Tag
  dialog can't be dismissed while it saves, a second click closes the sort
  menu, the list column's resize handle moves from the width you see, and
  Browse says when the file dialog can't open.
- 2026-10-02 **Delete all messages finishes whole and spares a running
  import.** A failure part-way left the conversations deleted and the rest
  in place. While an Import Run was uploading, the delete removed the
  attachment files of the run's next batch, and that batch stored its
  messages without their attachments. The delete now succeeds or fails as
  one step, waits for a batch in progress, and leaves the attachment files
  on disk while the account has an Import Run going; they are removed by
  the next Delete all messages with no import running, or with the account.
- 2026-10-03 **The Sources panel dims the screen the way every other dialog
  does.** The shade behind it now follows the light or the dark theme instead
  of one fixed grey.

#### The server

- 2026-10-04 **Stopping the server stops the conversion it was running.** A
  server stopped with Ctrl-C or `docker stop` while it made a browser copy
  of a video left that conversion running after the server had stopped,
  using the computer for nothing. It now stops the conversion, removes the
  part-made copy, and makes the copy again when it next starts. Stopping
  the command that rebuilds the copies does the same.
- 2026-10-04 **A video's browser copy plays in every browser.** The copy
  the server made of a HEVC video, the format an iPhone records in, was HEVC
  as well, which most browsers cannot play. It is now H.264, which they all
  can. A photo or MP3 that every browser shows as it is no longer gets a
  copy it does not need.
- 2026-10-01 **Docker Compose runs as a real user when UID and GID aren't
  set.** It ran the container with an empty user and printed warnings.
- 2026-10-02 **Long conversations can be read to the end.** Messages past
  the 50,000th of a conversation could not be loaded.
- 2026-10-02 **Storage counts a shared attachment once.** One video attached
  to ten messages counted ten times in an account's storage and the Owner's
  totals.
- 2026-10-02 **A busy server no longer fails requests that only read.**
  While an import held the database, recording when a token was last used
  could fail the request itself.
- 2026-10-02 **`docker stop` lets requests finish.** The server finished
  requests in flight on Ctrl-C only, so stopping the container cut off an
  Upload or a Demo Account build.
- 2026-10-02 **The Demo Account is whole or absent.** A Demo Account build
  that was stopped part-way read as ready with part of its conversations; it
  is now removed and reported failed, so the next build starts clean. A build
  from Owner Home touches the Demo Account alone: it no longer converts other
  accounts' attachments or holds up their requests, and nobody can enter the
  Demo Account until it is complete. A reset that fails leaves the database
  as it was and usable, a Demo Data settings file with a key it does not
  use is refused, and the first group's opening line names its real title.
- 2026-10-02 **Media conversion finishes and reports failures.** A long
  conversion could hang for good, and a failure now says what ffmpeg said.
  A GIF whose type was written with capitals or extra detail is left
  animated instead of turned into a still picture. A Preview cut short by
  an interrupted run is written again, an attachment still uploading is
  left alone, and the server's `process-assets` command reports failure
  when a conversion failed. The server's `import` command fails when it
  can't read an entry in the directory, instead of leaving that conversation
  out.
- 2026-10-02 **Programs using the HTTP API get the answers its reference
  describes.** The Bearer scheme is read in any case, the health check
  answers a probe that accepts only text, a message is found by its id
  wherever it is, oversized and out-of-range requests are refused with the
  documented error, and the API reference pages carry the same headers as
  everything else.
- 2026-10-02 **`reset-demo` works on the Message Crate your configuration
  names.** It built the Demo Account into a database of its own choosing
  beside the configuration directory, whatever the configuration said, and then
  replaced the configuration file with one the server would not start with.
  It now rebuilds the Demo Account in the configured database and leaves the
  file alone. Before it puts the rebuilt database in place, it checks every
  row of every other account and the Server Settings, and refuses if any of
  them changed.
- 2026-10-03 **Rebuilding the Demo Account no longer holds up other
  accounts.** The rebuild deleted the old Demo Account in one step and then
  compacted the whole database file, and on the large Demo Data another
  account's Upload or edit could wait long enough to fail. The old Demo
  Account's messages are now deleted a few thousand at a time, so other
  accounts' changes go through in between. Only `reset-demo`, which runs
  while the server is stopped, still compacts the file; a new Message Crate
  also starts listening sooner.
- 2026-10-04 **Stopping the server during a Demo Account build no longer
  waits for all its Demo Data.**
  Stopped while a build was still making up its Demo Data, the server waited
  until all of it was written, which on the large set is the longest part of
  the build, and could leave a directory of part-written Demo Data behind. It
  now stops as soon as the conversation it is writing is done, and leaves
  nothing behind.
- 2026-10-04 **Every contact in the Demo Data's Address Book has a name.**
  One contact in the medium Demo Data's Address Book, and three in the
  large one, had a blank name. Loading the book could not name them, so
  their numbers stayed under Unknown. Every contact in it now has a name,
  and building the medium Demo Account names all 75.
- 2026-10-04 **The Docker image no longer sets environment variables the
  server never reads.** The image set `MC_DB`, `MC_DATA_DIR` and `HOSTNAME`,
  and changing them changed nothing. The database and the data directory come
  from `[paths]` in the configuration, and the address the server listens
  on from `[server]`, as they always did.
- 2026-10-04 **Programs using the HTTP API get a conversation's first and
  last message times once each.** A conversation carried its last message's
  time twice. When no message was left once duplicates are set aside, it
  sent one of those times with no value rather than leaving it out. It now
  carries its first and last message times once each, and leaves both out
  when there is no message to date them.
- 2026-10-04 **Programs using the HTTP API can read a Contact Group,
  Message Tag or Saved Search by its id.** Creating one answered with the
  address that holds its id, and reading that address was refused. It now
  answers the item as the list shows it, and answers "not found" for one
  that belongs to another account.
- 2026-10-04 **Removing or changing messages right after an import no
  longer fails with "no such table: messages".** It failed now and then
  when an import had just finished on the same server.
- 2026-10-04 **A Demo Account that fails to build on a first start is
  removed and leaves nothing behind.** On a first start, a Demo Account that
  failed to build then could not be removed, and its files stayed behind.
- 2026-10-04 **The configuration reference states each request body limit as
  the server applies it.** It said the attachment size limit was also the
  limit on every other request body. It limits only an attachment upload.
  Logging in, creating an account, claiming a Message Crate, an address book
  load and every other request each have a limit fixed in the server, which
  the page now lists.
- 2026-10-04 **A Preview cut short is made again without `--force`.** A
  Preview left part-written by a stopped `process-assets` run was kept and
  shown as it was until someone ran the command with `--force`. Every run
  now checks each Preview against its contents and makes a damaged one
  again, and removes the part-written files a stopped run or import left in
  the attachment directories.
- 2026-10-04 **How the desktop app checks that its server started was
  reworked, with nothing visible.**
- 2026-10-04 **The server's `reset-demo` command checks more of what it
  must leave alone.** Before it puts the rebuilt Demo Account in place, it
  checks that nothing else changed. That check now also covers the Audit
  Trail of deleted accounts, what a search finds in other accounts'
  messages, and the files in other accounts' directories. The reset stops if any
  of them changed. On a database of about 1.3 million messages the check
  takes about 14 seconds.
- 2026-10-04 **Deleting attachments no longer holds up everyone else.**
  Emptying the Trash, deleting a conversation or all of an account's
  messages, and the clean-up at the end of an import deleted every file
  while keeping all other changes waiting. With many files on a slow disk,
  another person's import, a sign-in or media conversion could wait 15
  seconds and fail. The files are now set aside in a moment and deleted
  afterwards, while everything else goes on.
- 2026-10-04 **Checking on an attachment upload no longer reads the whole
  file again.** When the file being uploaded was already stored, asking
  how far the upload had got, sending one of its parts or cancelling it
  read and checked every byte of the stored file first, and threw the
  answer away. For a large video that was the whole file on every check.
  Those steps now read only the upload's own record.
- 2026-10-04 **The server refuses an attachment directory setting that is a
  path.** The configuration's `assets_dir` and `assets_converted_dir` each
  name one directory inside every account's directory, but an absolute path
  was accepted there. It put every account's attachments in one directory,
  and `reset-demo` failed with "prepared reset state is incomplete". The
  server now refuses to start when either is absolute, contains a
  separator or a `:`, starts with `.` (as `.` and `..` do), ends in `.` or
  a space, or is empty, or when both are the same name in any letter case,
  and the message names the setting.
- 2026-10-04 **A damaged Preview whose original is gone is no longer
  shown.** When `process-assets` found a Preview that does not match its
  contents and the original it was made from was missing, it could not make
  the Preview again, and the attachment went on showing the damaged one.
  The run now deletes that Preview, so the attachment shows as one with no
  Preview, and still reports it among the failures.

### Upgrading

- The database format changed. **An existing Message Crate is rebuilt
  empty on first start and its messages must be imported again.**
- If your configuration file sets `asset_max_bytes` under `[server]`, delete
  the line, because the server refuses to start with it. Set the limit under
  Owner Home → Server Settings instead. Any other key the server does not
  use, a misspelt one included, now stops it at startup too.
- `DEMO_DATA` is no longer read, because every new Message Crate starts with
  the Demo Account. The `admin` owner Demo Data used to create is gone:
  create the Owner on the login card, and delete the Demo Account from
  Owner Home if you don't want it.
- A Message Crate that ran on Postgres has to move to SQLite: export its
  messages first, start the new server, and import them again.
- If your configuration file has a `[database]` section, delete it. The
  server now refuses to start with it. If you start the server with
  `--db-url`, remove that flag; the database file is named by `db` under
  `[paths]`, or by `--db`.
- A vCard (`.vcf`) file and a contacts CSV from a phone or another app no
  longer load as an Address Book. To name many contacts at once, use
  **Export** on the Contacts screen, fill in the file, and load it under
  Settings.
- If you load contacts from the command line, the server's `import-contacts`
  command and the `--contacts` and `--overwrite-contacts` options of `import`
  are gone. Load the Address Book in the app instead.
- Each message's id from SMS Backup & Restore, GO SMS Pro, SMS Backup+,
  iMazing, OpenExtract and WhatsApp is now made differently. A backup already
  imported by an earlier 0.10.0 build is stored a second time if you import
  it again: use **Delete all messages** in Settings first, then import it.
- A Saved Search that uses `source:sms` is refused. Edit it to
  `source:sms-backup-restore`.
- Photos and files imported by an earlier 0.10.0 build are not shown.
  Each account now keeps them in one directory, `data/<account_id>/assets/`,
  and the server no longer reads the directory an earlier build made for each
  source, such as `data/1/imessage/`. **Delete all messages** leaves those
  old directories on disk. To bring the photos back:
  1. Use **Delete all messages** in Settings.
  2. Delete each `data/<account_id>/<source>/` directory.
  3. Import those backups again.
- The server's `process-assets` command no longer takes `--source`: it
  makes previews for every attachment of each account.
- If you have a program that reads a conversation from the HTTP API, it
  must read `first_message_at` in place of `date_range_start` and
  `last_message_at` in place of `date_range_end`. The old names are gone.
  Expect both to be missing when the conversation has no message left once
  duplicates are set aside.
- An Import Run left waiting at a Staging Review or at its Media Stage by an
  earlier build can't go on, and says its Staging did not finish. Discard it
  and start the import again. Do the same with a paused Apple Messages run
  from an earlier build: its staged files don't say which reactions are
  yours, so yours would be stored as someone else's.
- An Address Book exported by an earlier build calls its fifth column
  `handle_type`, and loading it is refused. Rename that column to
  `identity_type` in the file, or export the Address Book again.
- Message files exported by an earlier build are refused when you import
  or convert them: each phone number and email address in them is now
  written as an identity, and the old files say "handle". This holds for
  JSON, JSONL, CSV, EML and mbox exports, and for an Import Run an earlier
  build left paused. Export the backup again with this build, then import
  or convert the new files; discard a paused run and start the import again.
- Message files exported before reactions moved onto the message they react
  to are refused when you import or convert them, rather than read with their
  Apple Messages reactions lost. This holds for JSON, JSONL, CSV, EML and mbox
  exports, and for an Import Run an earlier build left paused. Export the
  backup again with this build, then import or convert the new files; discard
  a paused run and start the import again.
- Message files exported before a message could be marked Deleted in the
  source app or Unsent are refused when you import or convert them, rather
  than read with the mark lost. This holds for JSON, JSONL, CSV, EML and mbox
  exports, and for an Import Run an earlier build left paused. Export the
  backup again with this build, then import or convert the new files; discard
  a paused run and start the import again.
- Message files exported before an edited message kept its earlier versions
  are refused when you import or convert them, rather than read with those
  versions lost. This holds for JSON, JSONL, CSV, EML and mbox exports, and
  for an Import Run an earlier build left paused. Export the backup again
  with this build, then import or convert the new files; discard a paused run
  and start the import again.
- `reset-demo` no longer writes a configuration file, and reads the one given
  with `--config`. If an earlier `reset-demo` replaced your configuration
  file, the server stops at startup with a missing `[server]` section: put
  your own file back.

## [0.9.0] - 2026-09-22

The release that gives a vault an owner, a trash you can take things back out
of, and a search that reads the same everywhere.

### Features

- **A command-line import that was killed can be cleared from
  the command line.** An import stopped mid-run leaves its session open, and
  no later import for that account can start until it is discarded. The
  vault server now has `imports discard --account <account>`, which discards
  the account's open session and says which one it was, or that there was
  none. The refusal names the command, so you no longer need the desktop
  app's Import screen to get unstuck.
- **The vault owner can see how much the vault holds.** Owner
  Home's Dashboard now shows the vault's totals across every account: its
  storage total and how many messages, attachments, conversations and
  contacts it holds. An account's Storage tab shows its conversation and
  contact counts beside its message and attachment counts. These are counts
  only: no conversation, contact or message is named to the owner.
- **The contact list shows when you last heard from each
  contact, and can sort on it.** Every contact row now carries the date of
  the newest message that contact sent you, and the sort menu gains Last
  Heard From beside First Name and Last Name, newest first. A message you
  sent them, or one someone else sent in a group chat, does not count: the
  date is when they last wrote. Contacts you have never heard from sit at the
  end whichever way the list runs. The vault's contact list takes the same
  key: `GET /v1/contacts?sort=-last_heard`, and each contact carries
  `last_heard_at`.
- **The vault owner sees an account's Storage as its holder
  does.** Open an account from User Accounts and its Storage tab now shows its
  message count and storage total, every import and export it has run, and
  its largest attachments by name and size. Opening an import shows its
  counts, timings and issues, and how many contacts it created and changed.
  Who those contacts are, and which conversation a file is in, stay with the
  account. The account's Profile tab
  shows when it last logged in and which app it connects with.
- **Every screen can tell you which version it is.** Settings →
  System shows the version of the app you are using, in the browser and in the
  desktop app. The vault owner's Settings shows the vault's version and
  its schema fingerprint, and an account's Profile tab shows the owner which app
  the account connects with, the desktop app or the website, and its version.
  A build between two releases carries the commit it came from, such as
  `0.9.0+343fe0d8`, so two dev builds can be told apart. When an app and its
  vault come from different releases, the app says so in a line under the
  header, "This vault is 0.10.0. This app is 0.9.0.", and the owner sees it
  marked on that account's Profile. Nothing is blocked: the vault serves every app
  whatever its version.
- **User Accounts shows when each account last logged in.** A
  Last login column next to Status, in your own time zone, or "Never" for
  an account nobody has logged in to yet. Logging in, claiming the vault and
  registering all count; a password change does not.
- **Export can write just the conversations a search finds.**
  The Export screen opens with a scope: Everything, as before, or Search,
  which shows a box for a search in the same language as the search bar and
  exports only what it finds. `in:#19,#22` exports those two conversations
  and nothing else. Opening Export while browsing a list of conversations
  starts in Search with that list's search already filled in.
- **The stop in an import run is a review, and it shows how many
  messages each identity sent.** "Staging Approval" and "Media Approval" are
  now "Staging Review" and "Media Review", and a waiting one reads "Awaiting
  approval". Identities is a table: each address the backup sent from, how
  many of the staged messages it sent, whether it is on your profile, and
  "Add to profile" at the end of the row for one that is not. "Files over the
  limit" says "Skip vault upload" beside it instead of a sentence above the
  list. Each stage's title has a rule under it, "Cancel this import" looks
  like a button before the pointer reaches it, and the attachments'
  "Operation" is "Action".
- **An import run and its approvals are one list.** The run
  screen is the list of stages, and each approval is a row in that list where
  the run stops, with the decision inside it. There is no separate approval
  screen to open and come back from. Each stage's row holds what that stage
  made, one fact per line: Staging has the staging directory, conversations
  and messages, and the attachments' operation, count and total size. The
  Staging Approval shows contacts as Existing and New, the size limit per
  file (50 MB, which no screen showed before), and the files over it, which
  open to each file and its size. With Convert or Compress it adds estimates
  in three groups: Likely within limit, May exceed limit, and Not audio or
  video. The Media Approval shows what is true after Media, not how it
  compares with the estimate. The import log link sits in Upload's row and
  appears once Upload starts, so it no longer opens onto a file that does
  not exist yet. A finished run offers "View imported conversations", "View
  modified contacts" and Back; errors are in one table under the list.
- **Import is one screen that fills in as the run goes.** The
  form collapses into "what you asked for" once the run starts, each of the
  three stages (Staging, Media, Upload) adds its result underneath, and the
  finished run leads with where to go next: the conversations it added, the
  contacts it touched, or another import. Both approvals are the same
  screen; it opens on its own when a stage finishes and has a link back to
  the run. A run keeps working, and keeps waiting at an approval, while you
  are on another screen, and the Import entry in the sidebar carries a badge
  while a run needs you. The two approval screens no longer say "gate".
- **A vault has an owner.** A fresh vault now asks you to create
  its owner before anything else, and that owner is the one account that
  manages the vault: create accounts, disable them, reset a password, delete
  someone's messages, and decide whether strangers may sign themselves up. The
  owner has no messages of their own and cannot read anyone else's — the
  account list shows a name, a message count and a storage total, and nothing
  of what those messages say. There is exactly one owner and it cannot be
  deleted. If you forget its password, `create-owner` and
  `reset-owner-password` on the server put you back in.
- **New accounts are closed by default.** A vault admits nobody the
  owner has not admitted, until the owner turns on public registration. An account
  the owner creates has to replace the owner's password the first time it
  logs in, so the owner never keeps knowing it.
- **Permanent delete, from the trash only.** Deleting a trashed
  conversation removes it, its messages, and any attachment no other message
  still uses. Deleting a trashed contact does what a phone does: the name and
  edits go, the contact becomes Unknown, and its conversations stay, showing
  the number. **Empty Trash** does both for everything in it.
- **Export history.** An export is recorded like an import: what it
  covered — everything, a search, or conversations you picked — and how much it
  handed over. Settings → Storage lists Export history beside Import history.
- **Convert**, a desktop tool under Settings that rewrites a folder
  of already-exported files into another format without touching a backup or
  the vault.
- **Times read in your zone.** Your account carries a time zone,
  chosen at setup and changeable afterwards, and every message time, day and
  year is shown in it.
- The vault server writes a proper log — one line per request, the
  full reason behind any internal failure, and a warning for work it could not
  finish.
- **Find in conversation**, and years that page like every other
  list.
- Make a Contact Group straight from a group conversation.
- Import accepts owner email addresses for SMS Backup+, and the two
  Android SMS sources share one form.
- **A trash you can undo.** Conversations and contacts can be set
  aside and taken back. Nothing in the trash is deleted, a trashed conversation
  can still be opened and read, and lists leave the trash out unless asked.
  Trash has its own advanced search.
- **An import names the contact.** When a backup knows someone's
  name and the vault does not, the import puts that name on the contact. A name
  you type, or load from an address book, replaces one an import supplied; a
  later backup spelling it differently does not.

### Fixes

- **An unread Apple Messages message no longer claims it was
  read on 2001-01-01.** Messages stores no read time for a message nobody
  has read, and the reader turned that empty value into the earliest date
  Apple's clock can express, so every unread message imported from a Mac or
  an iPhone backup carried a read receipt from the start of 2001. An unread
  message now carries no read receipt; a read one keeps its real time.
- **A finished import no longer leaves its staging folder
  behind.** Every import wrote a copy of the backup's messages and
  attachments into the staging directory and left it there after the upload,
  so each import added gigabytes to the folder. An import that succeeds now
  deletes its staging folder, the import log with it; the run's record under
  Settings → Storage → Import history keeps its counts, timings and errors. A
  failed import still leaves the folder in place.
- **An attachment gets the same filename on every computer.** The
  date at the front of an attachment's filename is now the message's time in
  UTC. It used to be the time zone of the computer running the export, so the
  same backup exported on two computers named its attachments differently.
- **Searching every conversation for a word answers at once.** A
  word or phrase typed into Messages without an `in:` scope, and
  `messages:0` or `first-message:` on Contacts, took ten seconds to several
  minutes on a vault of 600,000 messages. Both now answer in well under a
  second with the same results.
- **Loading an address book again keeps your Contact Groups.** A
  contact the book created and that is still in the file keeps its group
  memberships, its conversations and its place in Import History; only its
  name and phone numbers change to what the file now says. A contact the file
  dropped is removed as before. A conversation with a number the book had
  supplied also survives a reload, where it used to be deleted with the number.
- **Reloading the website while the vault is down no longer logs
  you out.** The Login screen shows Disconnected as before, and when the vault
  answers again you go straight back in without typing your password. A login
  the vault itself rejects still asks for the password.
- **A contact with no name shows who it is.** The contact list,
  the contact's own panel and the Trash show its first identity, in italics,
  where they used to read "(unknown)" on every row. Its Contact Groups now
  say Unknown, and it no longer appears under No group as well.
- Long-running vaults no longer grow in memory for every file ever
  uploaded and every username ever tried.
- A failure now says what actually went wrong — the step that
  failed and the file or database error under it — rather than the outermost
  message alone.
- The published Docker image is built with the compiler the project
  tests against, not whatever the base image happened to carry.
- An obfuscated export no longer carries the original vendor data
  alongside the substituted text.
- A trashed contact is set aside rather than gone: an import that
  meets one of its numbers attaches to it and leaves it in the trash, and
  contact counts leave the trash out.
- Only the newest connection check may speak for the login card,
  so a slow answer for an old address cannot overwrite a newer one.
- Importing a file the vault cannot read explains what is wrong —
  which version the file is and which the vault reads, or which line is bad —
  instead of "internal server error".
- Message Vault Settings no longer shows an address green when it
  has not tried it. An address typed but not tested reads **Not tested**.
- The login and profile-setup pages no longer show a scrollbar on
  a screen tall enough to hold the card, so opening a dropdown stops shifting
  the card sideways.
- Profile setup refuses a phone number or address already in the
  list, marks the row that repeated it, and compares numbers regardless of how
  they are written. The same number on Text Message and on WhatsApp still
  counts as two.
- Desktop import no longer fails partway through with a source
  mismatch.
- Import errors group identical problems into one row with a file
  count, instead of one row per file.

### Design

- **The desktop app says what it ships from others.** Apple
  Messages are read by a separate program, the Apple Messages reader, which is
  free software under the GNU General Public License. Settings → About now has
  a Third-party software note naming it, with links to its source and license
  for the exact version you are running, and every installer carries the
  license text beside the program.
- **Owner Home has room to grow.** The side panel reads
  Dashboard, Settings, User Accounts, Activity and Logs. Settings is what was
  Vault Settings; Dashboard, Activity and Logs are named and empty for now.
  User Accounts is down to who, their status and their last login: the app, the
  message count and the storage total moved into the account's own Profile and
  Storage tabs.
- **Settings, Account shows an account's status and
  permissions.** Permissions lists Import messages, Export messages, and
  Delete messages and attachments. You can see what your account may do; the
  vault owner sets it. The owner changes an account's status and permissions
  from that account's Settings, and User Accounts now shows each status
  without the Import, Export and Delete columns.
- **Changing the vault owner's password asks for the current
  one.** The owner's account reaches every other account, so Settings,
  Account has a Current password field for the owner, and the vault checks
  it before it stores the new password. Every other account changes its
  password as before. The owner's Settings are now Account, Profile and
  Appearance: System and Convert work on messages, which the owner does not
  hold, and Profile no longer asks the owner for handles.
- **A gear in each User Accounts row opens that account's
  Settings.** It appears at the left of the row while the pointer is in it.
  The column headings are bold with a line between them, and every other row
  is a shade lighter.
  The screen is the one the account holder sees, with the Account, Profile and
  Storage tabs. Reset password, Delete messages and Delete account moved
  there from the Actions column, which is gone. You can read an account's
  profile and how much it stores; you cannot change the profile or see what
  is stored. Each account shows its preferred name under its username, and
  the search bar matches either. Your own account now leads the list, and its
  gear opens your own Settings.
- **Owner Home looks like the screen every account sees.** The
  product name, a search bar and the account button now run across the top.
  The search bar narrows User Accounts by username. The side panel lists
  Vault Settings, then User Accounts. The owner's password and appearance
  moved to Settings, under the account button, which is also where Log out
  now is.
- **The vault owner's screen is Owner Home, with a side panel.**
  The owner lands on it at login. Its side panel lists User Accounts first,
  then Vault, Password and Appearance, in place of the tabs across the top.
  In User Accounts, an account's status is a dropdown (Active or Disabled)
  instead of an Enable/Disable button, and both Add account and Reset
  password ask for the password twice and save only when the two match.
  Resetting a password now does just that: the person's current session
  carries on, and they keep the new password until they change it themselves.
  The vault no longer makes anyone replace a password the owner chose at
  their next login.
- **An import brings back a contact you had trashed.** Until now
  an import that met the handle of a trashed contact attached to it and left
  it in Trash, so someone you set aside once never appeared in Contacts again
  however many newer backups you imported. A backup that still holds the
  person means you still talk to them: the import now discards the trashed
  contact, name, group memberships and handles included, and makes a new
  contact from the backup, the way a first import would. The forecast of new
  contacts shown before an import counts them. To keep someone out for good,
  delete them from Trash. Why: `docs/adr/0013-an-import-replaces-a-trashed-contact.md`.
- **An Import Run says what it did to each contact.** The run
  records, as it goes, whether it created a contact, created one in place of
  a trashed contact, named one that had no name, or added a handle to one,
  and the run's record under Settings → Storage lists the contacts with that
  reason. The new and changed counts come from the same record instead of
  being guessed from timestamps afterwards.
- **Importing SMS Backup+ mail reads one message per file, and
  nothing else.** Message Vault briefly also read a second kind of `.eml` — a
  whole conversation written out as a dated transcript in one mail. That shape
  is not something SMS Backup+ produces, and every message in the only known
  collection of them was already present as ordinary SMS Backup+ mail, so
  reading it added a second copy of messages the vault already had. Support for
  it is gone. Importing a folder of SMS Backup+ mail is unchanged.
- **The HTTP interface was rebuilt on one set of conventions.**
  Every list pages and sorts the same way — the browse lists and the ones you
  curate alike, with no list left answering a bare array — every failure comes
  back in the same shape with a link to a page explaining that kind of failure,
  every created thing answers with its address, and every response carries an
  id you can quote when reporting a problem. Logging in, and everything to do
  with accounts, each moved to one address serving everyone, with the vault
  deciding what a given caller may see rather than the address saying it. A
  single message can now be read by its id, so a search result links to the
  message rather than to a position in a list. Parameters that no longer did
  anything are gone — nothing asks you to name your account when your key
  already says it — and import history sorts the same way export history does.
  This matters if you wrote something against the interface yourself; nothing
  in the app or the desktop app changes.
- The vault and the desktop app now convert media with the same
  code, so a video converted on import and a preview generated later can no
  longer differ. Previews are better quality than before.
- Import progress is reported by the exporters directly rather than
  read back out of their log text, so the progress bar can no longer be broken
  by a wording change. An encrypted iPhone backup now narrates its setup steps
  instead of sitting on "Reading backup…".
- The desktop app reads Apple Messages through a small separate
  program shipped beside it, because the library it uses carries a licence that
  cannot be combined with the app's. Nothing changes on screen.
- Release builds of the server and the desktop app are optimised
  and stripped, so they are smaller and faster.
- **One search language.** The search box, saved searches and the
  export filter are compiled by the same code, so a query means the same thing
  everywhere it is typed.
- The vault accepts the packaged desktop app without being
  configured to. A vault built from source used to refuse it in a way that
  looked like an unreachable server.
- Importing is substantially faster — messages, attachments and
  reactions are written in batches rather than one database call at a time.
- Import lists one **iMessage** source with a choice of Mac
  Messages, iPhone backup, or jailbroken iPhone, and one **WhatsApp** source
  with a choice of Android or iPhone. Encrypted iPhone backups ask for the
  password in the form. Required fields are marked; optional ones say so.

### Upgrading

- The database format changed several times during this release. **An existing
  vault is rebuilt empty on first start and its messages must be imported
  again.** There is no migration before the first stable release.
- If your configuration file sets `asset_hash_threshold_bytes`, delete the
  line. The setting did nothing and the vault now refuses to start with it.
- An obfuscation seed is exactly 64 characters — the length the exporter prints
  when it generates one. Shorter seeds are no longer accepted.
- The old desktop interface built with Slint has been removed. The desktop app
  is the one built with Tauri.

## [0.8.3] - 2026-08-25

### Fixes

- The published Docker image can finish building its sample inbox on systems
  where it previously failed partway through.

## [0.8.2] - 2026-08-25

### Fixes

- The published Docker image includes the files it needs to generate its sample
  inbox.

## [0.8.1] - 2026-08-25

### Fixes

- The published Docker image builds again. The 0.8.0 image failed to build.

## [0.8.0] - 2026-08-25

### Features

- A grey, green or red light beside the server address on the login screen,
  so you can see whether the vault is reachable before trying to connect.
- Settings → System applies changes to the import staging folder and the ffmpeg
  folder immediately, with no Save button, and reports whether ffmpeg was found.
- The Contacts list shows which range of contacts you are looking at in a pill
  at the bottom of the panel, always visible.

### Fixes

- Opening a contact from a message thread no longer flashes an empty Loading
  row before showing the real details.
- The panel divider between the navigation and the list can be grabbed again,
  and drags up to a wider maximum.
- Packaged desktop builds can connect to a vault out of the box. Release builds
  were previously blocked unless the vault was configured for them by hand.
- Editing a contact's name discards the draft on click-away, Tab or blur, and
  saves on Enter.
- The import staging folder no longer nests an extra folder inside the one you
  chose.

### Design

- Login leads with Connect or Log in. Extracting and converting files moved
  off that screen; importing a backup after logging in is unchanged.
- Closing the desktop window logs out, so the next launch asks you to log in.
- Pushing an import to the vault is considerably faster — larger batches, less
  repeated checking of files the vault already has, and more work overlapped.
- The navigation panel is width-draggable, and the conversation list can shrink
  away entirely on a narrow window so the thread stays readable.
- The sidebar section previously called Thread Tags is now **Message Tags**.
- Internal rework across the server, the libraries, the exporters and the
  command-line tools, with no change to what any of them produce. One
  behavioural difference: a KnugiHK binary placed in a custom tools folder is
  no longer found by WhatsApp Android export.

## [0.7.3] - 2026-08-13

### Fixes

- Packaging corrections for the 0.7 release.

## [0.7.2] - 2026-08-13

### Fixes

- The Docker image builds again after a removed folder was still being copied.

## [0.7.1] - 2026-08-13

### Fixes

- The Windows application icon.

## [0.7.0] - 2026-08-13

The release where Message Vault became one product: a vault you log in to and
browse, rather than a set of tools that write files.

### Features

- **Browse your messages in the app.** Conversations, contacts, threads,
  attachments with thumbnails, a lightbox and inline video, date jump links,
  and search.
- **Contacts.** A contact drawer with an editable table of the numbers and
  addresses that reach a person, name aliases, advanced search with date
  operators, and the ability to browse a contact's conversations from the
  drawer.
- **Guided import** with a live progress summary, per-stage timings, contact
  name review, and an import that continues past a conversation it cannot read
  instead of stopping. A finished import is saved as a group you can go back to.
- **Accounts and login**, with profile setup, appearance themes, and a danger
  zone for deleting your messages or your account.
- **API tokens** with scopes, for scripting against the vault.
- A sample inbox you can try the vault with, seeded on first start.

### Fixes

- Hardening across the exporters and the vault: attachment paths that tried to
  escape their folder, digest and date handling, authentication, tokens and
  cross-origin rules.
- Long lists no longer slow the app down; the sidebar, contacts and
  conversations are all paged and virtualised.

### Design

- The desktop app and the website are one React application, built on React
  Aria and Tailwind, so dialogs, drawers, progress bars and form fields behave
  consistently and work with a keyboard and a screen reader.
- The separate `message-vault-rs` repository was merged in, so the vault server
  and the tools that feed it live together.
- A message's transport (iMessage, SMS) is recorded separately from the
  platform a number belongs to.

## [0.6.0] - 2026-08-04

### Features

- **Guided vault import** in the desktop app: a form that walks through the
  credentials and the backup, with readable progress as it uploads.
- Attachments upload in parts, so a large file no longer has to succeed in one
  go.

### Fixes

- Import logs stay responsive under heavy output.
- Login explains an insecure-to-secure address mismatch instead of failing
  obscurely.

## [0.5.0] - 2026-08-02

### Features

- Push exported messages straight into a vault from the desktop app.
- Read contacts from a vCard file or a contacts CSV.

### Fixes

- The saved settings file is written with restricted permissions, and vault
  fields are kept when the app closes.

## [0.4.1] - 2026-07-30

### Fixes

- The Windows app no longer opens a console window behind it.

## [0.4.0] - 2026-07-30

### Features

- **A desktop application**, replacing the command-line-only workflow, with a
  form-based screen per task and a log you can watch.
- Errors are shown against the tab that produced them and can be dismissed.

### Design

- Uploading to a vault is much faster: files the vault already holds are
  detected before being sent, imports are batched, and the work overlaps.

## [0.3.0] - 2026-07-29

### Features

- Releases ship as self-contained archives per platform, so nothing has to be
  built to try the product.

## [0.2.0] - 2026-07-29

### Features

- **One common message format** underneath every backup type, so an iPhone
  export and an Android export produce the same thing.
- **Export to the format you want**: JSON Lines, JSON, CSV, EML, MBOX, or the
  Android restore XML, with JSON the default.
- **Convert an existing export** into another format without the original
  backup.
- Media handling and obfuscation apply to every format rather than to some of
  them.

### Fixes

- Staged attachments are cleaned up after being embedded in mail or XML output.

### Design

- The documentation site was rebuilt as end-user guides organised by the kind
  of message you are exporting, rather than by internal structure.

## [0.1.0] - 2026-07-18

The first release: command-line tools that read a phone backup and write CSV.

### Features

- Exporters for iMessage, SMS Backup & Restore, and related Android backups.

### Fixes

- Conversations with unknown participants are kept rather than dropped.
- MMS media is kept when a message's layout only partially matches.
- An owner phone number is required, so messages can be attributed correctly.

---

Installable builds also appear on
[GitHub Releases](https://github.com/messagecrate/message-crate/releases).
