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

## [0.11.0] — in development

### Features

- 2026-10-10: **An attachment says why it has no copy a browser can show.**
  When the server tries to make the Preview of a photo, video or recording
  and cannot, opening it now says **No Preview could be made** and what
  ffmpeg said about the file, such as that the file holds data it cannot
  read, or that the original file is missing. It used to say only that no
  copy existed yet, as if one were still coming, and the reason was in the
  server's log alone. The reason goes away once a later try makes the
  Preview (#2170).

- 2026-10-10: **User Accounts shows which accounts have no password.** In
  Owner Home, the Status of an account that has no password now reads
  **No password** under it. Once such an account is active, anyone who
  reaches the server and knows its username logs in with an empty password,
  and the list used to give no sign of which accounts those were. The Demo
  Account has no password by design and is not marked (#2146).

- 2026-10-10: **Docker reports whether the server answers.** The Docker image
  asks the server every 30 seconds whether it is up. `docker ps` adds
  `(healthy)` after `Up` while the server answers and `(unhealthy)` after
  three missed answers in a row, where it used to show only `Up`, even for a
  server that had stopped answering. A proxy or orchestrator in front of the
  container can read the same state. The first five minutes after a start
  are not counted as unhealthy, so the first start, which builds the Demo
  Account before the server answers, does not read as a fault. The check asks
  the server at the port it listens on, so a server that the config or
  `serve --bind` puts on a port other than 8080 reads healthy too. A
  container started for one command, such as
  `docker compose run --rm server reset-demo`, reads healthy while the
  command runs, rather than unhealthy after five minutes. So does a container
  that runs no server at all, such as a shell (#2132, #2541, #2542).

- 2026-10-09: **An iMazing import keeps reactions, replies, deleted messages
  and edits.** Each reaction in an iMazing export is kept with its emoji and
  whether the account holder gave it. The export names the person who
  reacted by display name alone, with no phone number or address, so a
  reaction is not matched to a contact. The server does not yet keep the
  name, so another person's reaction reads as from "Someone". A reply is linked
  to the message it quotes when that message is in the same conversation of
  the export, and stays a reply without a link otherwise. A message deleted
  on the phone before the export is marked **Deleted in the source app**. An
  edited message is marked **Edited**, and because iMazing keeps only the
  final text, **Edited** opens one line, **Earlier version not in the
  backup**, with the time of the edit; a search never finds the message by
  the text it lost. The database format changed: an existing Message Crate
  is rebuilt empty on first start and its messages must be imported again
  (#2030).

- 2026-10-09: **A WhatsApp import brings each reaction in under the person
  who reacted.** A WhatsApp conversation from an Android phone or an iPhone
  now shows its reactions, each stored under the reactor's phone number, or
  under their internal WhatsApp id when the backup has no number for them.
  Your own reactions are yours. Reactions that were taken back are left out.
  The copy of the reactor names and emoji that a conversation file kept
  beside each WhatsApp message (`reactions` in `source.fields`) is gone,
  because the reactions themselves are on the message (#1646).

- 2026-10-08: **The Import form says when a program the import needs is
  missing, with Try again.** A WhatsApp import needs wtsexporter, and
  **Convert** and **Compress** need ffmpeg and ffprobe. When one hasn't been
  downloaded or its download failed, the Import form names it and gives the
  reason, with **Try again**, which downloads it at once, and a link to
  Troubleshooting. A WhatsApp import can't start without wtsexporter. An
  import with **Convert** or **Compress** still starts, as before. An import
  started while its program is still downloading waits for it, and its
  progress line says so, such as "Waiting for the wtsexporter download: 12 MB
  of 30 MB (40%)". When that download fails, the run fails with the reason
  and names the troubleshooting section (#1053).

- 2026-10-08: **The desktop app downloads ffmpeg, ffprobe and wtsexporter
  by itself.** Each time it starts, it downloads in the background whatever
  is missing from its Tools Directory, with no button and nothing to wait
  for. ffmpeg and ffprobe aren't downloaded when both are already installed
  on the system path. Each program comes from a pinned release of its project,
  wtsexporter from Message Crate's own copy of the WhatsApp Chat Exporter,
  and every file is checked against a checksum the app carries. A file that
  doesn't match is deleted. A program in the Tools Directory that isn't the
  pinned one is replaced, but only once the new file has passed its check,
  so with no internet the old one is still used. Settings → System shows
  each download's progress, or why it failed, and a failed download is
  tried again at the next start (#1053).

- 2026-10-08: **A phone number written without its country code can be given
  its country.** The import form has a **Phone's country** under
  **Processing Options (Advanced)**: every number the import writes without a
  country code, such as `07700 900123`, is read as a number in that country,
  so it is the same person as `+44 7700 900123`. A number whose country nobody
  stated says **Country unknown** on the Contacts screen and under **My
  Identities**, with **Pick country**. Picking one gives the number its full
  form; when another identity already has that form, Message Crate says whose
  it is and asks before it merges the two, their one-to-one conversations
  with them (#1676).

- 2026-10-08: **Owner Home's Logs shows the server's log and the Import Run
  logs on this computer.** The owner picks the server's log or, in the
  desktop app, any Import Run's log on this computer, whoever ran it. An
  account opens its own run's log from the run's row in Settings → Storage,
  in the desktop app on the computer that ran the import. Both read the same
  way: newest line first, older lines as the list scrolls, a level filter
  (errors, warnings and up, or everything; it opens at warnings and up), a
  search that narrows the lines as it is typed, and a download of the log as
  it is. Each line of an Import Run's log now carries its time and level, the
  run's Import Errors rows are in it, and its first line names the run, the
  account that ran it, the server's address and the Message Crate's id.
  `GET /v1/server` answers that id, which the server writes when its
  database is made, so two Message Crates at one address, such as the
  desktop app's own server and a Docker one, are told apart (#1665).
- 2026-10-06: **A message recorded once to the second and once to the
  millisecond shows once.** Every message file now says whether each
  message's time has milliseconds or only whole seconds, as its backup app
  recorded it. iMazing, OpenExtract, GO SMS Pro's PDU files and SMS Backup+
  mails timed only to the second record whole seconds; the other sources
  record milliseconds. When one backup app holds a message twice, once to
  the second and once to the millisecond, such as two SMS Backup+ mails of
  one message, hiding duplicates hides the whole-second copy and shows the
  message at its time to the millisecond. When another backup app holds the
  message too, its copy may be the one shown, and it can have whole seconds
  only. A time to the millisecond that ends in `.000` counts as
  milliseconds. CSV exports carry it in a `time_precision` column, and mail
  exports in an `X-ME-Time-Precision` header. Message files exported before
  they said whether each time has milliseconds are refused, and the backup
  must be exported again with this build.
- 2026-10-05: **The newer backup decides when a message changed between two
  backups of one phone.** Every message file now says when its backup was
  made: an iPhone backup's own date, the date an SMS Backup & Restore file
  records, an iMazing export's date, or when the backup's files were last
  written for a source that records none. Importing two backups of the same
  phone, in one import or two and in either order, gives each message the
  newer backup's text, earlier versions and Deleted in the source app or
  Unsent mark, so a message recovered after it was deleted loses its mark,
  and a message unsent after the older backup reads as unsent. An older
  backup imported after a newer one changes nothing. Import details under
  Settings → Storage show the backup each import read and when it was made.
  Message files exported before they said when their backup was made are
  refused, and the backup must be exported again with this build (#1741,
  #1804).
- 2026-10-05: **A WhatsApp reply now names the message it quotes.** When
  the quoted message is in the same chat of the same backup, the reply is
  linked to it, as Apple Messages replies already were: a mail export threads
  the reply under that message, and a CSV or JSON export names it. A reply
  whose quoted message is not in the backup is still kept as a reply. The
  link needs a `wtsexporter` that records the quoted message's whole id,
  which only Message Crate's fork of WhatsApp Chat Exporter does, from its
  release `0.13.0-mc.2` on; a backup read by any other gives replies with no
  link.
- 2026-10-05: **Search finds unsent messages on their own.** On Messages,
  `unsent:yes` lists the messages their sender unsent, and `unsent:no`
  leaves them out. `deleted:yes` now lists only the messages deleted in the
  app they came from, and no longer brings unsent ones with them. A search
  for both writes both words, as `deleted:yes or unsent:yes`.

### Design

- 2026-10-10: **The Docker Compose files take away what the server does not
  need.** The container they start keeps no Linux privileges, cannot gain
  any, and cannot change its own files: the server writes only to the data
  volume and to a temporary directory in memory. Nothing in Message Crate
  changes, and a container scanner that checks for these settings now finds
  them. This limits what someone who got into the container could do. It
  closes no known hole (#2178).

- 2026-10-10: The way an import passes its log, its progress and its cancel
  around was reworked inside, with nothing visible (#2165).

- 2026-10-10: **An import is never sent back to a stage it has passed.**
  Message Crate now refuses to move an import that is under way back to an
  earlier stage, so a mistake in the app can no longer make the next resume
  start over at work already done. Nothing changes on screen (#2167).

- 2026-10-10: The check that ffmpeg is there before an import converts or
  compresses attachments was reworked inside, with nothing visible (#2166).

- 2026-10-10: **An SMS Backup+ import finishes the way the other imports
  do.** It now ends through the same steps as GO SMS Pro and the others.
  Nothing changes on screen (#2163).

- 2026-10-10: The Import Run and Export Run lists were reworked inside, with
  nothing visible (#2155).

- 2026-10-10: **SMS Backup & Restore and GO SMS Pro read a message's time
  the same way.** Both imports now share one check of which message times
  are readable, so they can no longer drift apart. Nothing changes on
  screen (#2154).

- 2026-10-10: **An Import Run keeps only the Import form choices it needs to
  resume.** The desktop app now stores a named list of the form's
  choices with each Import Run, rather than everything but the backup
  password and WhatsApp key. A secret field added to the form later is
  never kept in the database by mistake. Nothing changes on screen (#2142).

- 2026-10-10: **Apple Messages and WhatsApp check an optional path the same
  way.** The Import form's check of an optional path is now one check that
  both imports use, so they can no longer drift apart. Nothing changes on
  screen (#2141).

- 2026-10-10: **An SMS Backup & Restore group conversation is identified
  the way the SMS Backup+ and GO SMS Pro imports identify theirs.** The
  three imports now share one rule, so they can no longer drift apart. Each
  group conversation keeps the same messages, the same people and the same
  title. Upgrading says what this means for a backup imported before
  (#2139).

- 2026-10-10: **The Export screen names its format menu Output format.** It
  was Format there and Output format under Convert in Settings; both now read
  Output format, on screen and to a screen reader.

- 2026-10-09: **Every import hides duplicates.** An import from the desktop
  app never hid duplicates, so a message two backup apps both held, or one
  SMS Backup+ held to the second and to the millisecond, was shown twice.
  Every import now hides them, within one backup app and across apps, and
  nothing turns that off: the server's `import` command loses
  `--skip-dedupe`, and an Import Run made through the HTTP API takes no
  `dedupe` setting. Of the copies of one message, the one shown is the copy
  with more attachments, then the one timed to the millisecond, then the one
  from the import that brought more messages, then the one from the backup
  app imported first (#1969).

- 2026-10-09: **An SMS Backup & Restore import reads one `.xml` file.**
  The Import form's **Backup File** field takes the backup's one `.xml`
  file, where **Backup Directory** took a directory of them, because an
  import reads one backup. A second backup is a second import, and the
  server keeps the later copy of a message by the date each file records.
  A directory is refused. **Settings → Convert** refuses a directory that
  holds more than one SMS Backup & Restore file, naming each, and finds a
  backup the app wrote under any name. GO SMS Pro and SMS Backup+ still take
  a directory (#2009).

- 2026-10-08: **The desktop app finds an installed ffmpeg first, and its
  Tools Directory second.** ffmpeg and ffprobe are found among the programs
  installed on the computer, else in the app's Tools Directory, and both
  from the same place. wtsexporter is found only in the Tools Directory.
  The Message Crate the app starts looks in the same places, so the app and
  the Message Crate it starts convert with the same ffmpeg. **Settings →
  System → Media** has nothing to type any more. It shows the Tools Directory and, for ffmpeg, ffprobe and
  wtsexporter, where each was found, that it is missing and where to put
  it, or why it is not used (#1053).
- 2026-10-07: **Internal names were tidied.** Nothing changes on screen,
  on disk or in the HTTP API (#1715).
- 2026-10-05: **A run's log lists its Import Errors and notes under
  headings.** The summary at the end of an import or a Convert gives its
  Import Errors under an "Import Errors" heading and its notes under a
  "Notes" heading, and leaves out a heading with nothing under it. Each line
  is a sentence that names the item, such as "The file sms-2.xml could not
  be read in full: …", where it was `error: sms-2.xml: This file could not
  be read in full: …` before.
- 2026-10-05: **The server picks the order of the Messages list.** With no
  `sort`, `GET /v1/messages` now puts the best match first when the search
  has a free-text word to rank by, and the newest message first when it has
  none. It used to list the oldest first. Each page says which order it
  applied and which words it ranks by, in a new `search` key beside `items`,
  and the web app reads both from there: the sort menu offers Relevance only
  when words came back, and the list draws those words in bold. Nothing
  changes on screen, except that a search the web app and the server once
  read differently now shows the server's reading. Picking Relevance now
  leaves `sort` out of the address, so the next search starts in the
  server's order.
- 2026-10-05: **A message's reply count is counted when it is read.** It is
  the number of replies Message Crate shows that quote the message, so a
  reply hidden as a duplicate no longer counts, and a reply that quotes a
  copy hidden as a duplicate counts for the copy shown. The web app shows
  no reply count, so nothing changes on screen.
- 2026-10-05: **A released Docker image is tagged by its version only.** A
  release no longer also tags its image `sha-` and the commit. That tag now
  belongs only to images built by hand from a branch, which show the commit
  in their version, so one `sha-` tag always names one image. Pull a
  release by its version, such as `0.11.0`, or by `latest`.
- 2026-10-05: **An Export keeps its record of fetched attachments under a
  new name.** The desktop app's Export keeps a small record in the directory
  it writes, so a later Export there does not fetch the same attachments
  again. That record now has a new name; one with the old name is ignored and
  can be deleted, and attachments already in the directory are still not
  fetched again.
- 2026-10-07: **An Upload's report says Import.** The report an Upload
  writes beside its run, and the name the server keeps for the program that
  ran each Import Run the desktop app starts, now say import where they said
  push, the word Message Crate no longer uses for an Import Run. A report
  with the old name is not read and can be deleted (#1926).

### Fixes

#### Accounts, Settings and screens

- 2026-10-10: **A screen the server refuses shows its error at once.**
  When the server refused to show something, because it was not found, not
  allowed, or asked too often, the website and the desktop app asked a second
  time before showing the error, which held the error back a second or more
  and, when the server had said it was being asked too often, asked once
  more. They now show the error after the first answer, and ask again only
  when the server could not be reached or failed on its own side (#2169).
- 2026-10-10: **The website and the desktop app no longer follow a
  redirect with your Session.** If a misconfigured proxy in front of the
  server answered with a redirect to another address, the website or the
  desktop app followed it, and a browser or system web view built before
  the Fetch standard's November 2022 rule against this sent your Session
  along. The server never redirects, so both now refuse any
  redirect: the login card shows the server as **Disconnected**, which
  points at the proxy instead of sending your Session somewhere else
  (#2294).
- 2026-10-10: **The Demo Account can no longer make API Tokens.** Anyone
  who entered the Demo Account could make one, even one that never expires,
  and walk away with a way back in that outlived the visit. Every visitor
  shares the Demo Account, so each one also saw the tokens the others made
  and could revoke them. Its Settings now say it makes no API Tokens, and the
  server refuses one (#2148).
- 2026-10-10: **No API token works on the owner's account.** The owner
  cannot make an API token, but a token on the owner's account that got
  there another way, such as in a restored database, used to import into
  and export from the owner's account. The server now refuses it as if it
  had never issued it. The owner's entry in User Accounts also used to
  claim import, export and delete, which the owner never holds; it now
  reports none (#2147).
- 2026-10-10: **Logging in to a disabled account no longer tells a guesser
  the password was right.** A disabled account's login used to say the
  account was disabled only when the password was right. A wrong password got
  the usual "invalid username or password". So someone guessing learned which
  guess worked. That password works again once the owner enables the
  account. Every refused login now gets the same answer. The
  Audit Trail still records that the login was refused because the account
  is disabled (#2145).
- 2026-10-10: **A password typed into the username field no longer reaches
  the Audit Trail.** A refused login for a username that matches no account
  kept the text exactly as typed for 90 days, so a password typed into the wrong
  field, which happens when a browser fills it in, was there for the owner
  to read. The Audit Trail now keeps the text only when it could be a
  username, and otherwise says the login was refused for something that is
  not a valid username (#2135).
- 2026-10-10: **The time zone picker and Sources end an empty-state
  message without a full stop.** The time zone picker said "No time zone
  matches." when nothing matched what was typed, and Sources said "No
  source data available." for a conversation with no source data. Both now
  end without a full stop, like the empty lists in Contacts and Trash
  (#2469).
- 2026-10-10: **API Tokens, Identities, Export History, Import History,
  the largest attachments in Storage, the Audit Trail and the log viewer
  end an empty-list message without a full stop.** They ended theirs with
  one, where Contacts and Trash did not. "No API Tokens yet.",
  "No identities yet.", "No exports recorded yet.", "No imports recorded
  yet.", "No attachments with sizes yet.", "Nothing recorded yet.", "No line
  matches the search." and "No line at this level." now end without one
  (#2428).
- 2026-10-10: **The contacts picked on Contacts and a message's attachments
  in search results are counted with a separator.** The heading over the
  picked contacts said "1234 contacts selected", and a search result's
  attachment count read "📎 1234" with the tooltip "1234 attachments"; each
  now writes "1,234", with the separator of the language the browser or
  desktop app is set to (#2417).
- 2026-10-10: **Trash writes an empty list the way Contacts does.** Trash
  said "No contacts match this search." with a full stop where Contacts said
  "No contacts match this search" without one. Every message Trash shows for
  an empty list or a search that finds nothing, from "Trash is empty" to
  "No contacts in Trash", now ends without a full stop, like the empty lists
  in Contacts (#2415).
- 2026-10-10: **Removing an identity from a contact writes a large count
  with a separator.** The confirm dialog said it would unlink "1234
  conversations"; it now writes "1,234 conversations", with the separator of
  the language the browser or desktop app is set to (#2397).
- 2026-10-10: **Trash, Export History and the Audit Trail write a large
  count the way Storage does.** Trash wrote "1234 conversations" where the
  storage screens wrote "1,234 messages". Every count in Trash, the count of
  conversations and messages picked by hand in an Export Run's scope in
  Export History, and the contacts changed and removed by an address book
  load in the Audit Trail now carry the separator of the language the browser
  or desktop app is set to (#2240).
- 2026-10-07: **The login card names the server it is connected to.** The
  card said only **Connected**, so a person could not tell whether the desktop
  app was on its own Message Crate or on one on another computer. It now reads
  **Connected to 127.0.0.1:8080** for the app's own, **Connected to
  localhost:8080** in a browser on the same computer, or **Connected to
  192.168.1.20:9000** for one elsewhere: the host and port as the address
  gives them, without the scheme. Connecting, Disconnected and Not tested name
  the server the same way, and **Connection Status** on the **Server Address**
  screen names the address in the field (#1973).
- 2026-10-07: **Conversations, Contacts and Trash keep their list in a
  narrow window.** In a window 390 px wide the list column shrank to 1 px and
  the right pane was cut off at the window's edge. Beside a list the
  navigation panel now gives way first, down to 160 px, the list stops at
  220 px and the right pane at 320 px. In a window narrower than the three,
  the area under the header scrolls sideways to reach the right pane, and the
  header stays where it is. A photo in a conversation is no wider than its
  message. Settings, Import and Export still fit the window. Phone layouts
  come later (#1722).

#### Desktop app

- 2026-10-10: **The desktop app no longer adds an unused, ready-made way to
  call its commands to its window.** Nothing changes on screen (#2177).
- 2026-10-10: **A new SMS Backup+ export clears out every conversation
  directory of the one before it.** An SMS Backup+ export that stopped part
  way could leave an empty conversation directory behind, and the next
  export into the same directory kept it. A new export into that directory
  now removes every conversation directory an earlier SMS Backup+ export
  wrote, empty or not (#2299).
- 2026-10-10: **A one-to-one conversation that does not say who it was
  with is exported under the name `unknown`.** Such a conversation was
  exported to a file named only by its extension, such as `.json`. macOS
  and Linux hide a file named that way. An SMS Backup+ export wrote its mail
  loose in the export directory, and a later export into that directory
  never removed it. Its files and its SMS Backup+ directory are now named
  `unknown`, as an untitled group with no members is named `group_unknown`
  (#2459).
- 2026-10-10: **A downloaded ffmpeg is not reported as broken by
  mistake.** On Linux, the check that a just-downloaded ffmpeg or ffprobe
  runs could wrongly report it as not running. The check now tries again,
  and a file still being written is still reported as not running (#2031).
- 2026-10-07: **The desktop app remembers the server address.** An address
  entered under **Change server address** was saved only with the login.
  Logging out, or a login the server no longer accepted, forgot it. The next
  start was back on `http://127.0.0.1:8080` and started the app's own Message
  Crate. While a login was saved, the app instead opened on **Server
  Address** at every start and waited for **Use this address**. The address
  is now a setting of its own that logging out leaves alone. The app opens on
  the login card for the saved address and starts its own Message Crate only
  when the saved address is `http://127.0.0.1:8080`. **Change server
  address** still changes it, or goes back to the app's own. The website
  keeps a changed address the same way, and its **Server Address** screen
  offers **Use this website's own Message Crate** to go back. Going back to
  an address, in the app or the website, restores a login saved for it
  (#1972).
- 2026-10-05: **The Message Crate the desktop app started stops when the app
  crashes.** Closing the app has always stopped it. When the app crashed or
  was ended from the task manager instead, its Message Crate went on running
  in the background, converting attachments, until the computer restarted.
  It now notices within two seconds that the app is gone and stops the way
  it does on a normal stop, finishing what it was answering first.
- 2026-10-05: **Closing the desktop app stops a video conversion its Message
  Crate was running.** The app stopped its Message Crate, but a video it was
  converting went on converting in the background until it was done, and
  the result was thrown away. The conversion now stops with the Message
  Crate. On Windows the same happens when the app crashes, so there the
  Message Crate stops at once rather than finishing what it was answering.

#### Importing

- 2026-10-10: **An attachment an import could not save is named in the
  Import Run's log.** It used to be written where no log of Message Crate
  showed it (#2165).

- 2026-10-10: **The Import form tells a path it may not read from one that
  is not there.** A backup path the desktop app is not allowed to read, such
  as a directory macOS protects until the app has Full Disk Access, used to be
  reported as "This path does not exist.", which sent people looking for a
  different path when the fix was a permission. The form now says Message
  Crate isn't allowed to read the path, gives the reason the system gave, and
  says that on a Mac, Full Disk Access lets it read the path. On a Mac
  without Full Disk Access, Apple Messages on this Mac now fills in the
  Messages database anyway, so that message shows at once. When the app
  could not check a path at all, the form says so rather than calling it
  missing. A stopped Import Run whose directory or backup the app may not
  read is no longer offered for discarding as if it were gone or changed
  (#2173).
- 2026-10-10: **Converting or compressing attachments no longer follows a
  symlink.** When the Media Stage of an Import Run converts or compresses
  attachments, a symlink among the staged attachments used to be followed:
  the files of a linked directory elsewhere were converted and replaced
  where they lay, and a link back to a directory above it made the Media
  Stage repeat its walk until it failed. A symlink there is now skipped, so a
  linked directory is no longer converted, and only the attachments that
  are really in the Import Run's directory change (#2262).

- 2026-10-10: **The Import form refuses a socket, a device file or a pipe
  in any of its path fields.** An Apple Messages or WhatsApp backup path,
  an Apple Messages attachments directory, an Apple Contacts database, or a
  WhatsApp database, contacts database or media directory that named a path
  that is neither a file nor a directory passed the form's check, and the
  import failed only after it had started. The form now gives the field's
  own message for such a path, the same one it gives for a file where a
  directory is needed or the reverse (#2539, #2562).
- 2026-10-10: **A large attachment no longer fails to import because the
  server said another request held its upload.** The desktop app sends a
  large attachment to the server in parts, and the server could refuse a
  part with "another request to this upload holds its lock" when nothing
  else was sending to that upload, and the import of that attachment
  failed. A part is now refused this way only while another request to the
  same upload is still running (#2510).
- 2026-10-10: **An SMS Backup & Restore import says "drafts or messages
  never sent" for what it skips.** It said "drafts or unsent messages",
  but Unsent is the mark on a message its sender pulled back after sending
  it, and those messages are kept. The count is of drafts and of messages
  the phone left in the outbox, failed to send, or queued (#2461).
- 2026-10-09: **Apple Messages from an older Mac or iPhone keep their times
  in whole seconds.** A Messages database from before macOS 10.13 and iOS 11
  stores each message's time in whole seconds, and the import said those
  times had milliseconds. It now says they are whole seconds, as it does for
  a message whose date the database holds in a form that could only be read
  to the second. A message also held with milliseconds by another backup of
  the same phone is then shown once (#1970).
- 2026-10-09: **Apple Messages from a newer Mac or iPhone keep their
  milliseconds.** Every Apple Messages time was cut to the second, though
  the Messages database holds it to the nanosecond. A message now shows
  its time with its milliseconds, as WhatsApp messages do (#1970).
- 2026-10-09: **A photo a later import fills in gets its Thumbnail.** When a
  backup was imported again and now held a photo or video that was missing
  the first time, the import gave the stored message its file but never
  queued it, so it got no Thumbnail or Preview, and the viewer did not open
  it, until `process-assets` was run by hand. The server now makes the
  Thumbnails and Previews of every attachment an import adds or fills in,
  whichever import first brought the message (#1946).
- 2026-10-09: **A file without a backup date imported beside a dated one
  gives the same result in any order.** When one import held a dated backup
  and a message file that says nothing about when its backup was made, such
  as Message Crate's own export of a conversation whose messages came from
  two backups, the order the files were read in decided what a later import
  could change. An Unsent mark from the undated file could be cleared by a
  later backup, or kept, and a later edit it recorded could be dropped
  against a newer backup already stored. Message Crate now keeps what a file
  without a date gave a message apart from the dated backups' date: its mark
  stays, and its text stays until a copy records a later edit, whichever
  file is read first and whether the files arrive in one import or several,
  until a dated backup says the same. A conversation exported while one of
  its messages holds such a mark or text carries no backup date, so
  importing the export again keeps those rules (#1989).
- 2026-10-09: **A failed WhatsApp import keeps what wtsexporter said.** When
  wtsexporter's output named a full disk, the import asked to free space on
  the Scratch Directory's disk. The output itself was lost. So a full disk
  elsewhere, such as the one that holds the system's temporary files, could
  not be told apart. The import still asks to free space on the Scratch
  Directory's disk. The Import Run's log now also holds everything a failed
  wtsexporter run printed, as warnings (#1938).
- 2026-10-09: **A resumed import drops a Staging Error about a file it then
  reads clean.** When Staging could not read a file of the backup, such as a
  mail file on a network drive that dropped, and the run was paused and
  resumed after the cause went away, the finished run still listed the file
  as unreadable. A resumed Staging reads the whole backup again, so once it
  has, its Errors and notes about the backup's files replace the earlier
  parts', and an Error stays only while its file still can't be read. A run
  resumed at a Review, Media, or Upload reads nothing of the backup again and
  keeps them (#1947).
- 2026-10-09: **WhatsApp people are imported with their numbers and names,
  and every group with its members.** A group message's sender had a name or
  a number, never both, some numbers were made from WhatsApp's internal ids
  and reached nobody, no group listed its members, and every one-to-one
  contact had no name and was listed under Unknown. Each sender now has the
  phone number the backup holds and the name from the phone's address book,
  or else the one they set in WhatsApp. A group's participants are its
  members, including those who never wrote, and anyone else who wrote in it.
  A one-to-one contact has the chat's name. A person WhatsApp knows only by
  an internal id is imported under that id, with their name, and is listed
  under Unknown until given an address. A WhatsApp file that another
  version of wtsexporter wrote is refused, naming Message Crate's own (#1092).
- 2026-10-09: **Owner Home's Logs panel shows how each import went.** The
  lines an import writes as it reads its files, writes them into the account
  and hides duplicates never reached the **Logs** panel. They do now, and so
  do the lines of the Demo Account's import when it is built or rebuilt. An
  import that was running when the desktop app crashed could also stop
  part-way while writing one of those lines. It no longer does. The server's
  `import` and `reset-demo` commands still print the same lines as they run
  (#1945).
- 2026-10-08: **A number written without its country code is no longer read
  as a US number.** A UK backup's `07700900123` and `+447700900123` were two
  identities for one person, so their one-to-one conversation split in two,
  and SMS Backup+ counted a group member with one number as having two. Such
  a number now keeps its digits until its country is known, from the import
  form or picked on the Contacts screen, and then it is one identity with its
  full form. A ten-digit number without `+` is not taken for a US number
  either: an import of a US phone's backup states the United States as the
  phone's country to read `555 555 0100` as `+1 555 555 0100` (#1676).
- 2026-10-08: **Two reads of encrypted iPhone backups at once no longer
  break each other.** Opening an encrypted backup decrypts its file list to
  one fixed name in the computer's temporary directory, so two reads running
  together, such as an Apple Messages import and a WhatsApp import from
  encrypted backups, wrote over each other's copy and one failed. The file
  list was also left behind when a read stopped on an error. It now goes into
  the read's own directory under the Scratch Directory, which is deleted when
  the read ends (#788).

- 2026-10-07: **A message an import edits is no longer hidden behind a copy
  of its old text.** When an import set not to hide duplicates gave a stored
  message the text of a later edit, or added an attachment to it, the message
  stayed hidden behind another backup's copy of what it said before, so a
  search for its new text found nothing. A copy hidden behind it stayed
  hidden too, though their texts no longer matched. Such an import now checks
  the duplicates of the messages it changed, and of the copies around them,
  whatever its own setting. The messages it adds stay as they came. A stored
  message that only imports with dedupe off brought stays as it came too,
  even when a later import changes its text.

- 2026-10-05: **A WhatsApp import from Android that fills the disk holding
  the Scratch Directory stops with the free-space sentence.** The encrypted
  WhatsApp backup is decrypted into the Scratch Directory, and its decrypted
  size isn't known until it is written, so no check can measure it first.
  When that disk filled, the run stopped with `wtsexporter`'s own error. It
  now stops with "Not enough space on the disk that holds the Scratch
  Directory", the sentence every other free-space check gives, whichever
  language Windows is set to and whether the disk was full before
  `wtsexporter` started or filled while it ran. The part already written is
  deleted with the rest of the run's working files, as it was before. The
  WhatsApp guide says the decrypted database can run to several GB and where
  it is written.

- 2026-10-05: **A resumed Staging no longer leaves behind an Error it put
  right.** When an iPhone import stopped after an attachment could not be
  decrypted, for example on a full disk, and the resumed Staging then wrote
  that attachment's conversation with the attachment decrypted, the finished
  run still listed the attachment as not decrypted. Such an Error now waits
  until its conversation is written, and goes when a resumed Staging writes
  the conversation again. An Error about a conversation written before the
  stop stays, because the resumed Staging does not read it again.

- 2026-10-05: **An import no longer stores a WhatsApp id as an email
  address.** A WhatsApp file that named a person by an internal id such as
  `123456789012345@lid` and gave it no type stored that id as an email
  identity, so the identities list called it `email`. An import now stores
  every address with an `@` on WhatsApp as `other`, wherever it appears: the
  chat, its members, the senders, the reactions and your own address. An
  email address on Text Message is still an email address, because iMessage
  reaches one.

- 2026-10-07: **Messages you sent to Ada that an Apple Messages backup kept
  in no conversation sit with the ones she sent.** Such messages sat apart
  from Ada's, in "Unknown recipient", mixed with everyone else's, though the
  backup names Ada as their recipient. They now sit in Ada's conversation of
  orphaned messages, with the ones she sent. That conversation is titled
  "Ada · Orphaned". Messages that name nobody, ones you sent with no
  recipient recorded and ones received with no sender or from one of your
  own addresses, sit in "Orphaned · Unknown person". `kind:orphaned` still
  finds both kinds, and the word "orphaned" now finds them by their titles
  too.

#### Browsing and search

- 2026-10-10: **The left panel highlights Contacts on a Contact Group page.**
  A Contact Group page, **No Contact Group** and **Unknown** list contacts,
  but the left panel highlighted no Browse row on them, where a Message Tag
  page highlights Messages. Each now highlights Contacts. A screen reader
  also hears which row of Messages, Contacts, Trash, Import and Export is the
  current page (#2480).
- 2026-10-09: **The left panel's rows are one size, and its sections one gap
  apart.** A named Contact Group, Saved Search, or Message Tag was drawn
  smaller than the rows around it, such as **Unknown** and **No Contact
  Group**, and the space above Saved Searches and above Message Tags differed
  from the space above the other sections. Every row is now the same size,
  and each section sits the same distance below the one above it.

- 2026-10-07: **A hit found by an earlier version says so in the Messages
  list.** A search finds a message when one of its earlier versions holds a
  searched word, but the row showed only the final text, so a word only an
  earlier version held showed nowhere, and the hit looked like a wrong result
  until it was opened. Searching `imprudent delighted` on the Demo Account
  listed "You have delighted us long enough." with "delighted" in bold and
  nothing for "imprudent". The row now keeps the final text and adds a muted
  line for each earlier version it quotes, here "Earlier version: So
  imprudent a match on both sides!", with the searched words in bold. The
  row quotes the newest matched earlier version, and, for each searched word
  that neither that version nor the final text shows, the newest matched
  version that holds the word (#1785).

- 2026-10-07: **A contact's and an identity's counts include orphaned
  messages.** Since orphaned messages got conversations of their own, the
  counts that split a person's conversations and messages into direct and
  group left them out, so the two figures added up to less than the total.
  A contact, its identities, the selected contacts on Contacts and the
  identities on Profile now count orphaned conversations and messages as a
  third figure, and the three add up to the total. The orphaned figure
  shows only when it is not zero.

- 2026-10-05: **A search no longer finds an Unsent message by the text it
  hides.** An Unsent message reads "Unsent" and nothing else, but when an
  earlier import stored it with its text, a search for a word of that text,
  or of an attachment's file name, still listed it, with nothing on the row
  to say why. `body:`, `subject:`, `filename:` and the attachment words did
  the same. A search now finds a message by what it shows: `unsent:yes`,
  `from:`, the conversation and the date still find an Unsent message, and a
  message Deleted in the source app is still found by its text.

- 2026-10-05: **A message keeps the milliseconds of its time.** Message
  Crate kept a message's time to the whole second and dropped the
  milliseconds that WhatsApp, Apple Messages and SMS Backup & Restore
  record. Two messages sent within one second could then show
  in the wrong order. The time is now kept to the millisecond, a
  conversation lists its messages in the order they were sent, and an
  export written from Message Crate keeps the milliseconds too. A backup
  that records whole seconds, such as iMazing or OpenExtract, lists the
  messages of one second in the order the backup gives them, as before.

#### Contacts and identities

- 2026-10-10: **Contacts calls a search that finds nothing a search.** When
  the text in **Search contacts** matched no contact, the list said "No
  contacts match this filter". It now says "No contacts match this search"
  (#2290).

- 2026-10-09: **A person who reacts to a message is the same contact as when
  they write.** An import could take a person in a conversation's list and
  the same person reacting to a message for two identities, and put the
  reaction on a second contact with no name. That happened when the backup
  said the person's address was a name while it reads as a phone number, such
  as `5550123`. Message Crate now works out what every address is itself,
  from the service and the address, for the people in a conversation, the
  senders of its messages and the people who react alike, and no longer reads
  it from the exported file (#1933, #1959).

- 2026-10-08: **Adding or removing an identity updates the conversations with
  yourself already imported.** Notes imported before your number was added as an
  identity still listed you among the conversation's people. They also kept you
  as your own contact, in **Contacts** and under **Unknown**, while the
  conversation was already titled with the account's name. Notes imported while
  the number was an identity showed the bare address once it was removed, with
  no person or contact behind it. Saving the identities now stops listing you
  among a conversation's people, groups included, and the other people stay. The
  contact an import made for you goes too, unless it has a name, is in the Trash
  or in a Contact Group you made, or something else refers to it. Removing the
  identity again lists you as before, with the name the backup gave, and a
  conversation that is no longer with yourself gets its person back, with a
  contact. Messages are not changed (#1662).

#### The server

- 2026-10-10: **One account can no longer fill the server's disk with
  unfinished uploads.** A large attachment is sent in parts, and the parts
  of an upload that was never finished stayed on disk for a day. Nothing
  limited how many such uploads one account could have open, so one account
  could leave enough of them to fill the disk that every account on the
  server shares. An account may now have 128 uploads in progress at once.
  The next is refused until one of them is completed, is stopped by the
  program that sent it, or has sat untouched for a day (#2176).
- 2026-10-10: **An attachment the server cannot read is written to its
  log.** Before it skips an upload because an attachment is already
  stored, the server reads the stored copy to check it. When that read
  failed, for example on a failing disk or after a permission change, the
  server asked for the attachment again and said nothing about why. The
  attachment is still sent again, and the server's log now names the file
  it could not read and the error (#2171).
- 2026-10-10: **A request too large for the server reads the same
  everywhere.** When the server refuses a request for being over its size
  limit, the message now always reads "the request body is too large". It
  used to read three ways depending on what was sent: "request body too
  large" for an attachment, an import or an address book sent without a
  stated size, "a part of this upload is at most" a number of bytes for a
  part of a large attachment, and a technical sentence from the web
  framework for some other requests (#2283).
- 2026-10-10: **The server's log and the desktop app's Import Run logs no
  longer act on a terminal that shows them.** A file name in a backup or a
  tool's output can carry a terminal control character, such as the escape
  that clears the screen. Both logs wrote it as it was, so opening the file
  with `cat`, `less -r` or `tail` could clear the screen or rewrite what it
  showed. Both now write each control character escaped, such as `\x1b`,
  and the **Logs** panel and a run's row in Settings → Storage show it the
  same way (#2275).
- 2026-10-10: **A caching proxy no longer keeps a photo or video after its
  link has run out.** When a page shows an attachment, the server hands it a
  link that works for an hour, or until you log out. A caching proxy in front
  of the server could keep the attachment and hand it out again under that
  link after the hour or the logout. The server now tells every cache and
  browser not to store an attachment read through such a link, and tells
  shared caches not to keep any attachment at all (#2149).
- 2026-10-10: **No cache keeps your messages or your Session.** The server
  now tells every browser and proxy not to store what it answers: logging in,
  which hands out your Session, and every page of messages, contacts and
  settings.
  A browser could otherwise write those answers to its own cache on disk,
  and a proxy that ignores the usual rules could keep them and hand them out
  again. The website's own files are cached as before (#2295).
- 2026-10-10: **The guide to moving a Message Crate says to encrypt the
  copy.** The copy of the Docker volume that the guide makes holds every
  account's messages and attachments unencrypted, and the guide sent it on
  by USB drive, network share or `scp` without saying that it was
  unencrypted. It now encrypts the copy with `age` or `gpg` before it leaves
  the old computer, deletes every copy once the new computer's Message Crate
  is checked, and says the computer that runs the server should use
  full-disk encryption, such as FileVault, BitLocker or LUKS (#2161).
- 2026-10-10: **The Docker image no longer lets pages on port 5173 call its
  server.** The image's settings allowed web pages from port 5173 on the
  viewer's computer, the port a Message Crate developer's tools use, to call
  the server. Nothing in a release uses that port, so the only effect was
  that any page served on it could call a Message Crate in Docker, though it
  still needed a login to read anything. The image now allows only the
  desktop app and the website the server serves itself, as a Message Crate
  started by the desktop app does (#2144).

### Upgrading

- The **ffmpeg directory** field in Settings → System is gone, and the
  `MESSAGE_CRATE_BIN` and `WTSEXPORTER` environment variables are no longer
  read. ffmpeg and ffprobe beside the app, in a `lib/` directory beside it,
  or in the directory the field or `MESSAGE_CRATE_BIN` named are no longer
  found, and a WhatsApp import no longer finds wtsexporter on `PATH`. The
  desktop app downloads all three into its Tools Directory when it starts,
  ffmpeg and ffprobe only when they aren't installed on `PATH`, so nothing
  needs doing. Without a connection, Troubleshooting in the user
  guide says which files to put there by hand. If you start the server with
  a script, it takes `serve --tools-dir <directory>` for the same purpose
  (#1053).
- A WhatsApp `result.json` that a `wtsexporter` from before this release
  wrote is refused, because it records no group members and no sender's
  name beside their number. The desktop app replaces an older `wtsexporter`
  in its Tools Directory with Message Crate's own on its next start with an
  internet connection, except on Linux on ARM, where it has no download of
  its own and Troubleshooting in the user guide says how to install
  wtsexporter by hand. Run the WhatsApp import again from the backup rather
  than from an old `result.json` (#1092).
- A phone number written without its country code is no longer read as a
  US number. Import a US phone's backup with **Phone's country** set to the
  United States, or pick the country of each such number on the Contacts
  screen. If you have a program that creates Import Runs through the HTTP
  API, it states the country in `phone_country` (#1676).
- The database format changed. **An existing Message Crate is rebuilt empty
  on first start and its messages must be imported again.**
- A desktop app that was pointed at a Message Crate on another computer
  opens on this computer's own once after upgrading, because the address is
  now saved apart from the login. Enter the address again under **Change
  server address** and log in. From then on the app keeps it.
- Message files exported before replies moved onto the message itself are
  refused when you import or convert them, rather than read with their
  replies taken for plain messages. This holds for JSON, JSONL, CSV, EML and
  mbox exports, and for an Import Run an earlier build left paused. Export
  the backup again with this build, then import or convert the new files;
  discard a paused run and start the import again.
- If you have a program that reads messages from the HTTP API, it must read
  a reply from `reply_to`, which names the quoted message's `guid` and part,
  in place of `is_reply`, `thread_originator_guid` and
  `thread_originator_part`, and a message's reply count from `reply_count`
  in place of `num_replies`.
- A Saved Search that uses `deleted:yes` finds fewer messages than before:
  it leaves unsent messages out. Add `or unsent:yes` to it to find both.
- If you have a program that reads messages from the HTTP API, a message's
  `timestamp` and an earlier version's `edited_at` now carry three digits of
  milliseconds, such as `2015-03-12T18:04:22.250Z`, where they had none.
- Message files exported before they said when their backup was made are
  refused when you import or convert them. Export the backup again with
  this build. A program that reads the HTTP API finds the backup's date in
  a message's `backup_taken_at` and an Import Run's `backup_taken_at`.
- Message files exported before each message said whether its time has
  milliseconds are refused when you import or convert them. Export the
  backup again with this build. A program that reads the HTTP API finds it
  in a message's `time_precision`, `seconds` or `milliseconds`.
- Message files exported before Message Crate worked out each address's kind
  itself are refused when you import or convert them. Export the backup
  again with this build.
- An SMS Backup & Restore backup imported before this release and imported
  again afterwards makes a second copy of each group conversation, beside
  the one the first import made. To keep one of each, delete the older
  copy, or start from an empty Message Crate before importing again
  (#2139).
- If you have a program that reads conversations or messages from the HTTP
  API, it must read the title a conversation is shown by from
  `shown_title` in place of `label`, on the conversation list and on a
  message's `conversation`, and the export's title of a largest
  attachment's conversation in an account's storage from `group_title` in
  place of `conversation_title` (#2198).
- A Docker Compose file saved from this release starts only the image of
  this release or a later one. An older image fails to start under it,
  because it wrote a file outside the data volume at every start. Pin
  `bitrealm/message-crate` to this release or `latest`, and run
  `docker compose pull` before `docker compose up -d`, because Compose does
  not fetch a `latest` it already has (#2178).

## [0.10.1] - 2026-10-05

### Features

- **The conversation shows an edited message's earlier
  versions.** The line under an edited message reads like "4:56 PM · Edited",
  and pressing "Edited" opens the earlier versions under the bubble, each
  with the time it was written, and closes them again. They are closed when
  a conversation opens. A message opened from the Messages list that the
  search found only by a word an earlier version holds opens with its earlier
  versions shown, that version highlighted, and the line "Matched an earlier
  version" above them. Find does the same. Every source draws them the same
  way. The Demo Account has a few edited messages in its Apple Messages
  conversations.
- **Messages a backup kept without saying which conversation
  they were in now sit in conversations of their own, one for each person who
  sent them.** An Apple Messages backup can hold messages that belong to no
  chat. They used to arrive as one conversation named "orphaned", mixing many
  people's messages into one list and showing a person named "orphaned" in
  Contacts. Now the ones Ada sent are in "Ada · Missing recipient", with Ada in
  it and apart from your conversation with her, and the ones you sent are in
  "Unknown recipient". The same goes for messages you sent that an OpenExtract
  export does not name a recipient for, and the same text sent to several
  people in the same second is kept once for each of them. Search for these
  conversations with `kind:orphaned`; `kind:direct` and `kind:group` leave
  them out.
- **An edited Apple Messages message keeps every earlier
  version, and search finds it by any of them.** A message edited in Apple
  Messages is imported with its final text and each version before it, with
  the time each one was written. Searching Messages for a word that only an
  earlier version held finds the message. A later import leaves a message
  already stored as it is, with its text and earlier versions. Export keeps the
  earlier versions, so a conversation exported and imported again keeps them
  too.
- **The conversation and the Messages list show which messages
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
- **A message deleted in Apple Messages, or unsent, is kept and
  marked.** A message deleted in Apple Messages that its recently deleted
  list still holds is imported with its text and marked Deleted in the
  source app. A message its sender unsent is imported as Unsent, rather than
  as a line saying someone unsent a message. A message only partly unsent
  keeps what is left and has no mark. Search finds marked messages like any
  other, and `deleted:yes` or `deleted:no` on Messages narrows to them or
  away from them. Nothing is hidden. A later import of the same message
  that carries the mark adds it to the message already there. Export keeps
  the mark, so a conversation exported and imported again keeps it too.
- **A long conversation scrolls without downloading its photos,
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
- **Photos and videos get their browser copies after every
  import, without anyone asking.** Once an import ends, the server makes a
  small thumbnail of every photo and video it brought, and a copy every
  browser can show of each HEIC photo, HEVC video, voice note and other file
  browsers often cannot play. It works in the background, so the import
  finishes as soon as its messages are in, and a server stopped part-way
  finishes the rest when it starts again. Until now those copies existed
  only after someone ran a command on the server.
- **The server keeps its log in files the owner can read.** Docker and the
  desktop app's server now write their log to a `logs` directory beside the
  database, as well as to their output, so the lines that explain a failure
  are still there after a restart. It keeps at most 250 MB, in five files of
  50 MB, and deletes the oldest file when a new one starts. The owner can read
  it through the server's interface, newest first, narrow it to errors or
  warnings, search it, and download a whole file. No other account can. The
  log never holds a password, a token, a search, message text, an attachment
  or a contact's name, phone number or email address.
- **A search word the other list takes stays in the box.**
  Switching between Conversations and Messages with a word only one of them
  takes, such as `from:me` on Conversations, no longer shows an error. The
  word is underlined with a red wavy line and the list searches with the rest.
  Click the word to see which list it works in, and **Remove** it there if you
  no longer want it. Switching back searches with it again.
- **Notes you sent to yourself are their own conversation.** A chat
  with your own number or email, such as Apple Messages to yourself or
  WhatsApp's "Message yourself", imports with no one else in it and no
  longer adds you to Contacts. It goes by your display name, or by the
  address when you have none, and changes when your display name does.
  Search `with:me` to list exactly those conversations.
- **The Audit Trail narrows to a deleted account.** Owner Home's
  Account picker lists deleted accounts below the live ones, each by its
  old username and when it was deleted. Picking one shows only what
  that account did and what was done to it. Another account given the same
  username, before or after, keeps its own entries apart.
- **An Audit Trail of what each user did, and when.** Owner Home's
  Activity panel is now the Audit Trail: every login, session ending and
  refused login, every import and export, and every change to an account,
  newest first, with who did it and from which app. The owner reads every
  account's and can narrow it to one. Each person reads what concerns their
  own account under Settings, including what the owner changed. Nobody can
  edit or remove an entry, and an account's entries stay, under its old
  username, after the account is deleted.
- **One identity table, on the contact drawer and on an account's
  Profile.** An account's identities now show what a contact's do: the
  service, the address, when it was first and last heard from, and how many
  conversations, direct messages and group messages it takes part in. The
  columns line up under their headers, the sort arrow sits next to the
  label, and every row ends with a visible Remove. Adding an identity opens
  a small dialog instead of a permanent row under the table, and the dialog
  offers Email everywhere, so a contact can be given an email address by
  hand.
- **The Dashboard shows where a Message Crate's disk space goes.**
  Owner Home's Dashboard is now three sections. Contents is the card it
  had. Database shows the size of the database on disk, how much of it the
  messages take and how much the full-text search index adds, all measured
  by the server. Messages by account lists every account with its message
  count, its text and an estimated size on disk, split from the messages
  figure by each account's share of text, with a totals row so the split
  visibly adds up. Attachment files are counted under Contents, not in
  the database size.
- **A WhatsApp import knows which number is yours.** Every
  imported WhatsApp message now records the phone number your WhatsApp
  account is registered to, so its conversations count toward that identity
  in Settings. An iPhone backup carries the number, and Import reads it from
  there. An Android backup does not, so the Import form asks for it in a
  **WhatsApp phone number** field, pre-filled from your profile's phone; on
  iPhone the same field sits under Processing Options as a fallback for a
  backup without the number. The number is recorded on the messages and is
  not added to your profile.
- **Search contacts by what they sent you.** On Contacts, every
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
- **The desktop app is a Message Crate on its own.** The
  installer now carries the server and the website. When you open the app
  and nothing answers on this computer at its usual address, the app starts
  its own Message Crate, keeps its data in your system's app-data directory,
  and stops it when the app closes. A Message Crate already running there,
  such as one in Docker, is used as it is. Settings → System has an **Open
  data directory** button. Trying Message Crate no longer needs Docker.
- **Every new Message Crate starts with the Demo Account.** A
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
- **The Owner adds or resets the Demo Account from Owner Home.**
  Server Settings has a Demo Account card that adds it back after it was
  deleted, or puts it back the way it started, with a choice of the medium
  set or the large one of about 613,000 messages. No restart and no command
  line are needed. The Demo Account's page under User Accounts is read-only
  apart from Delete.
- **The Owner sets the attachment size limit.** The largest
  attachment the server accepts is now one number under Owner Home → Server
  Settings, 512 MiB until the Owner changes it. The desktop app reads it
  before Staging, so the Staging Review, Media and the Upload measure every
  file against the limit that is really in force. Before, the desktop app
  left out any file over 50 MiB of its own accord, whatever the server would
  take.
- **A conversation shows an attachment's Preview.** A HEIC photo
  or an HEVC video imported with Attachments → Copy now shows in any
  browser, once the server has made its Preview. Opening the attachment
  still gives the original.
- **The Address Book is a spreadsheet you export, edit, and load
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
- **The Demo Account has Contact Groups.** Demo Data is now built
  the way your own Message Crate is: its messages are imported first, and an
  Address Book then names the people in them and puts them in Family, Work,
  College, and Inactive.
- **The header names the account you are logged in as.** The
  username sits beside the account button for every account, the Owner
  included, so on a Message Crate several people share you can see whose
  messages are on screen.
- **Import and Export say when your account may not use them.**
  In the desktop app, an account the Owner has not allowed to import or
  export sees a message saying so in place of the form, instead of filling
  it in and being refused partway through. The Demo Account's Import screen
  says importing needs a personal account and offers **Log out**.
- **WhatsApp imports from an encrypted iPhone backup.** WhatsApp
  → iPhone on the Import form now has an **Encryption password** field, the
  same one iMessage has. The app sees that a backup is encrypted and asks
  for the password before the import starts. The encrypted backup you made
  for your iPhone messages now serves for WhatsApp too; a second,
  unencrypted backup is no longer needed.
- **Export and Convert write SMS Backup+ mail.** Choose **EML
  (SMS Backup+)** to get your SMS and MMS back as the mail SMS Backup+
  writes, one directory per conversation, which the SMS Backup+ import reads
  again and any mail program can keep. Other messages are left out, and the
  log says how many.

### Design

- **A program that reads Import Runs from the server finds each
  run's directory under a new name.** The old name read as the Staging
  Directory, which holds every run's directory. Nothing changes on screen.
- **Import says "Media stage" where it said "media step" or
  "Media pass".** An Import Run that stopped during Media offers to carry on
  with "The Media stage did not finish", and starting something else while
  Media runs names "the Media stage" as what is running. The import
  internals were renamed to match, with nothing else to see.
- **Every answer from the server carries the same fields.** A
  field with nothing in it is sent empty instead of being left out, so a
  program that talks to the server always finds the fields it expects.
  Nothing changes on screen.
- **An Import Run's log is kept for good, and scratch data has a
  directory of its own.** Each Import Run's log is written to the Logs
  Directory, named for the run, instead of the run's directory in the
  Staging Directory, so it is still there after the run ends. It now holds
  what Staging and Media said as well as the Upload, and the Import
  screen's link opens it there. The decrypted iPhone backup databases and
  the attachments read out of SMS backups go to the Scratch Directory beside
  it, instead of the operating system's cache directory, and what a stopped
  run left there is deleted the next time the app starts. The user guide's
  Settings → System page lists where the app keeps each of its directories.
- **Every Export and Convert gets a directory of its own in the
  Export Directory.** An export no longer needs a directory chosen before it
  starts: it is written into a new directory named for its date, time and
  format, such as `export-2026-10-04-1430-mbox`, in the Export Directory,
  which Settings → System names and opens. Convert does the same when no
  output directory is chosen. A directory can still be chosen for either.
  The JSON Lines an export converts from now wait in that directory instead
  of the Staging Directory, and are deleted when it finishes, leaving only
  the result; a failed or cancelled export deletes its directory, and one
  the app did not see to its end is deleted the next time it starts.
- **Screens, the user guide and the glossary say "directory"
  everywhere they said "folder".**
- **A reaction travels on the message it reacts to.** An Apple Messages
  tapback or emoji reaction is written into an export on the message it reacts
  to, with the person who reacted named, and an import stores it under that
  person. Export writes the reactions the server keeps the same way, so a
  conversation exported and imported again keeps them. Every app's reactions
  take this one shape; Apple Messages is the one app that writes them today.
- **The server sends an attachment a piece at a time.** A video or voice note
  can start playing before the whole file has arrived, and a player can jump
  to any point in it without downloading what comes before. The server also
  hands the app a short-lived link to one attachment, which works for an hour
  and ends when you log out, so a player built into the page can load it.
  Nothing on screen changes. The architecture notes record how attachments are
  shown: a small thumbnail in the conversation, the original or a
  browser-ready copy chosen by file type when one is opened, and the original
  whenever one is downloaded.
- **An account identity means ownership.** The Profile tab now
  says what the identities are for: your phone numbers and emails, which
  Import uses to determine which messages belong to you. The glossary and
  the architecture notes record the same distinction: a contact's identity
  means the person took part, an account's means the messages are theirs.
- **Profile Setup shows the identities already on your account
  in their own fields.** Phone numbers and emails the Owner added
  now fill the rows, where you can change or remove them before going on,
  instead of sitting in a line of text above them.
- **Shorter wording on two screens.** The screen that creates
  the Owner now opens with "An owner is required to create and manage
  users.", and
  the Display Name button in Settings reads Save without changing to Saved.
- **A contact's identities read the same as an account's.** The
  contact drawer now shows each identity in the form Message Crate stores it,
  a phone number in international form, and names an email address as
  Email, just as the Profile tab does. Message Crate counts both tables the
  same way. Under the surface, the server's interface and code were renamed
  to use the words the product uses, with nothing else to see.
- **The project moved.** The repository is now
  `messagecrate/message-crate`, the documentation is at
  <https://messagecrate.app/docs/>, the hosted product answers at
  <https://my.messagecrate.app>, and the Docker image is
  `bitrealm/message-crate`. Every error response's `type` URL now points at
  the new documentation host.
- **Message Vault is now Message Crate.** Every screen, every page
  of the documentation, every error message and the HTTP API reference use
  the new name, and the word "vault" is gone from all of them. One
  installation is "a Message Crate", the account that runs it is the
  "Owner", and the owner's installation-wide settings are "Server Settings".
  The desktop app's window and installers carry the new name.
- **Everything that was named after the old product has a new
  name, and nothing old still works.** The owner's routes are under
  `/v1/server`. Session and API tokens start `mc-user-` and `mc-api-`, so
  every existing token stops working and everyone logs in again. The
  database file is `data/messagecrate.db` and several tables are renamed, so
  an existing database is rebuilt empty and needs a fresh import. The
  Staging Directory defaults to `~/message-crate`. The compose service is
  `server`, and the desktop app installs as a new application beside any
  older copy.
- **The user guide starts with the desktop app.** It is now in
  two parts. Try Message Crate installs the desktop app and looks around the
  Demo Account. Your own messages creates the Owner and an account, backs up
  a phone, and imports it, all on the same Message Crate. Docker is no
  longer the first step.
- **The server runs on SQLite only.** A Message Crate could also
  keep its database on a Postgres server, which existed for a hosted service
  that is not built yet. That option is removed, so the server is simpler to
  run and to change. Postgres support comes back with the hosted service.
  The last of the server code written to choose between the two engines is
  gone too, with nothing to see.
- **Force reprocessing is gone from the Import form.** It changed
  nothing on a new Import Run, and on a resumed Upload it only sent
  everything again, which made the resume slower and the Duplicate counts
  higher.
- **The server refuses a configuration file it doesn't
  understand.** A section or key the server does not use, a misspelt one
  included, stops it at startup with the name and section of each, instead
  of being ignored.
- **A paused Upload is not finished.** Pressing the
  Upload's button now pauses the Import Run and keeps what it staged, and
  the next visit to Import offers to resume it, sending only the
  conversations not yet sent. Before, the run was recorded as completed, the
  staged files were deleted, and the conversations it had not reached were
  never imported. The run's report now puts every conversation in exactly
  one count, and lists the ones it left unsent. An Upload that fails,
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
- **One desktop job runs at a time.** An export won't start while
  an Import Run's job runs, and the reverse, and the screen says why. Before,
  the two jobs mixed up each other's progress, and one Cancel stopped both.
- **Every phone number in Demo Data is one reserved for
  fiction.** No Demo Data number can be dialled or belong to a real person.
  Demo Data messages also fall in the daytime, between 08:00 and 23:00,
  where most of them used to fall overnight.
- **Convert reads JSON Lines files only when they end in
  `.jsonl`,** the name Message Crate gives them. A file ending in `.ndjson`
  is no longer taken for one.

### Fixes

#### Importing

- **The Upload's report no longer says the Upload stopped.** A
  conversation the Upload did not reach before it was paused, or before the
  server refused its session, was listed with "the Upload was stopped before
  this conversation was sent". It now reads "the Upload ended before this
  conversation was sent".
- **An Import Run's log and the Upload write their last
  warnings as sentences.** A few lines in an Import Run's log started
  "Unable to", "Skipping" or "could not", and the desktop app wrote
  "Starting:" and "Done:" for each conversation of an Upload. Each now says
  what happened:
  - "Unable to remove decrypted temp file …" is now "The decrypted copy of
    attachment … at … could not be removed: …". An attachment whose decrypted
    copy could not be read says "Attachment … was decrypted, but its copy at
    … could not be read: …", and the run counts it in "Left out 1 attachment
    that could not be decrypted or read back".
  - "Unable to build contacts index: …" and "Unable to read a contacts
    source: …" are now "Contacts could not be read, so no contact is named
    from them: …" and "A contacts source could not be read, so no contact is
    named from it: …". "Could not decrypt Contacts database from iOS backup:
    …; continuing without contacts" is now "The contacts database in the
    iPhone backup could not be decrypted, so no contact is named from it: …".
  - "Skipping message (rowid=…, guid=…): …" and "2 messages skipped due to
    formatting errors." are now "Message … (row … of the messages database)
    could not be read, so it is left out: …" and "2 messages could not be
    read and were left out".
    "Chat ID 7 does not exist in chat table!" is now "A message names
    conversation 7, which is not in the messages database".
  - "could not read …" for WhatsApp's preferences is now "WhatsApp's
    preferences file … could not be read: …".
  - The desktop app's "Starting: chat.jsonl" and "Done: chat.jsonl (ok)" for
    each conversation of an Upload are now "Uploading chat.jsonl,
    conversation 2 of 5" and "Uploaded chat.jsonl", "chat.jsonl was not
    uploaded", or "chat.jsonl was uploaded before, so it is not sent again".
- **An Upload paused while its last request is in flight
  completes.** Such a pause left nothing to send. Every conversation still
  landed. Yet the desktop app kept the Import Run paused, and offered to
  resume a run with nothing left in it. A pause now keeps the Import Run
  paused only when it leaves a conversation for the next Upload, so this
  one completes. An Upload whose session the server refused always pauses,
  and its log now says "so the Upload paused" where it said "so the Upload
  stopped".
- **A journal line a crash cut inside a character no longer
  stops every later Upload.** Such a line stopped every Upload of that
  directory until its journal was deleted by hand. It is now skipped and
  named in the Upload's log like any other line the Upload cannot read.
- **A journal line the Upload cannot read is named in the
  Upload's log.** It went to standard error as "warning: journal … line … is
  corrupt (…). The affected entries will be re-submitted (server dedup is
  safe).", where a person reading a paused Upload's log did not see it. The
  log now says "Line 3 of the Upload's journal … could not be read (…), so
  the Upload skips it and may send again what it recorded. The server skips
  what it already holds." The desktop app's own line for an event no window
  received reads "The desktop app could not send the … event to its window:
  …", with no "warning:" in front.
- **The Upload's log writes its progress, skips, failures,
  timings and summary as sentences.** The log wrote shorthand such as
  "skip …: …", "files 10/25: …", "PROFILE … read_ms=… total_ms=…" and a
  "==== Summary ====" block of labels. It now writes lines such as "Did not
  upload …: …", "Finished 10 of 25 conversations. In the last 12.3s the
  Upload sent 10 conversations with 20 messages and 7.0 MB of Assets." and
  "The Upload completed in 1m00s." An Upload that left conversations for
  the next Upload ends with "The Upload paused after …", where it used to
  say it completed with errors.
- **An Import Run's log writes its warnings as sentences.** Lines
  written while reading a backup started with "warning:" or "media warning:".
  Each now says what happened:
  - "warning: attachment … could not be read: …" and "warning: failed to read
    attachment …: …" are now "Attachment … could not be read: …".
  - "warning: attachment … could not be decrypted: …" is now "Attachment …
    could not be decrypted: …".
  - "warning: attachment … not found in encrypted backup; skipping bytes" is
    now "Attachment … is not in the encrypted backup, so it is recorded
    without its file".
  - "warning: … could not be decrypted: …", for a file of an encrypted
    iPhone backup, is now "Backup file … could not be decrypted: …".
  - "warning: failed to remove temporary messages database at …: …" is now
    "The temporary copy of the messages database at … could not be removed:
    …", and the same for the contacts database.
  - The Media Stage's "media warning: 1 file could not be converted; …" is
    now "1 file could not be converted; …".
- **The Upload's log writes its warnings as sentences.** An
  attachment whose recorded SHA-256 did not match the file was logged as
  "WARN … sha256 mismatch for …: claimed …, got …", and a report the Upload
  could not write as "warning: write report …". The log now writes "…
  attachment … hashes to …, not the … its conversation file records. The
  Upload names it Asset …", "… the SHA-256 recorded for attachment … is not
  64 hexadecimal digits, so the Upload hashes the file instead", and "The
  Upload's report could not be written: …". When the Upload checks each
  SHA-256 before sending, it refuses the conversation instead, and the
  refusal starts with the same sentence the warning does.
- **The Upload's log names each attachment it sends as an
  Asset, in a sentence.** The log wrote "asset ok 3f2b…" for an attachment
  the Upload sent and "asset skip 3f2b…" for one the server already had. It
  now writes "Uploaded Asset 3f2b…" and "The server already holds Asset
  3f2b…".
- **When part of an Upload fails, the message names the Import
  Run or Asset in the words the app uses for them.** These messages used
  lowercase shorthand or a bare web address, and a refused completion of an
  Import Run named the run twice in one message. Now each one names what was
  being sent once, followed by what went wrong, whether the server refused it
  or the connection failed. The Upload's log names each batch of
  conversations by its Import Run the same way.
- **The Upload's log and the desktop app say the same
  sentences.** The log wrote "authenticated username=sam account=1",
  "using provided Import Run id=7", "Import Run id=7 source=sms",
  "skip_attachments=true (text-only import)", "session refused: stopped"
  and "session refused at login: stopped", while the desktop app showed
  other words for the same lines. Both now say "Authenticated as sam (1)",
  "Reusing Import Run 7 for sms", "Recording Import Run 7 for sms",
  "Skipping attachments (text-only import)", "The server no longer accepts
  this session, so the Upload paused" and "… so the Upload did not
  start".
- **One import of two backups keeps the attachments and
  reactions of both.** When one import carried two backups holding the same
  message, the message kept only the attachments and reactions of the file
  read first, so a Like only the newer backup held was lost. Two separate
  imports of the same files kept it. The message now takes the attachments
  and reactions of both, each one once, and a file one backup has and the
  other lacks fills in the missing attachment, as a second import does.
- **An import's log says its counts in words, not as
  `name=value`.** The Upload, the attachment conversion, and the server's
  import, duplicate check and `process-assets` command wrote counts such as
  "conversations=2 messages=2", "processed=1 skipped=0" and "promoted
  convs=1 parts=2 msgs=3". Each now reads like "2 conversations and 2
  messages", "processed 1 file, skipped 0 files" and "promoted 1
  conversation, 2 participants, 3 messages, 1 attachment and 0 tapbacks",
  singular for one. The server's `import` command no longer prints its
  duplicate counts twice, because its summary already gives them.
- **One import of two backups gives a message its later edit.**
  When one import carried an older and a newer backup of the same phone,
  and a message new to the Message Crate was edited between them, the
  message took the text and earlier versions of whichever file came first,
  so it could keep the older text. It now takes the newer backup's text and
  earlier versions in either order, and search finds it as an import of
  the newer backup alone would.
- **The server's import command names the line of a conversation
  it refuses for its source.** Before it imports a directory, the command
  reads every conversation's source. A conversation with no source, with a
  source name it does not accept, or with a second source in one file
  stopped the import without saying which line, and a source name it did
  not accept did not name the file either. Each now names the file and the
  line of that conversation, as every other refusal does.
- **A WhatsApp import keeps its working files out of the
  Staging Directory.** WhatsApp's files read out of an iPhone backup,
  decrypted when the backup is encrypted, sat in a directory inside the
  Staging Directory, and stayed there in the clear when the app was stopped
  during Staging. They now go in the Scratch Directory, are deleted when the
  run ends, and are deleted the next time the app starts if it was stopped.
  Before it reads them out, the import checks that the disk holding the
  Scratch Directory has room for them, and stops with the space it needs
  when it does not.
- **An Import Run whose session has ended logs you out and
  waits for you.** When your session ended while an import waited at a
  review, and you then approved it, cancelled it, or resumed an Upload, the
  import was marked failed with an error about recording its progress, and
  you stayed on screens the server no longer answered. It now logs you out
  and records no error. The import stays where it was, and Import offers it
  again when you log back in.
- **A newer backup brings a message's later edit.** When a
  message already in the Message Crate had been edited again on the phone,
  importing the newer backup kept the old text and the old earlier versions,
  because the import skips a message it already holds. A word only the new
  text held found nothing, and Export wrote the old text. The message now
  takes the newer backup's text and earlier versions, and search finds it by
  both. An older backup imported after a newer one leaves the message as it
  is.
- **An import's log and Convert's log say each count in the same
  plain words.** Converting and importing one SMS Backup & Restore backup
  used to word its counts two ways: Convert wrote "Skipped 1 message with no
  usable address", and an import wrote `skipped_unknown_address: 1`, printed
  even when the count was 0, along with "skipped 1 invalid-date rows" and
  "saved 1 attachments". Every count an import's summary or Convert's log
  gives now has one line, singular for one, such as "Skipped 1 message with
  an invalid date" and "Read 7 SMS", and a count of 0 is left out. A file
  that could not be read is an `error:` line in both. This holds for every
  kind of backup, not only SMS Backup & Restore.
- **An SMS Backup & Restore import names each message it kept
  with something left out.** A picture or other part whose data could not be
  read, and a character the backup wrote as a code that is not a character,
  such as `&#0;`, are left out and the message is kept. The run used to count
  them and say nothing more. Each such message is now a note on the run that
  names its file, its time and its `address` as the backup writes it, such as
  `/backups/smses.xml (message of 2014-05-22T15:51:40Z with +15555550101)`. A
  repeated copy of a message is named once, and a message the import skips
  is not named.
- **An iMazing import keeps each of two pictures sent in one
  second, and every picture stays with its own message.** When two photos
  with one file name arrived in the same second, iMazing saved them as
  `image0.jpg` and `image0 2.jpg`, and the import kept only the first
  message, because it compared the file name the rows gave and not the
  files. It now compares the files: two different pictures are two
  messages, each with its own picture, and two copies of one picture are
  still one message. Dropping that second message also moved every later
  picture of the import onto the message before its own. Each picture now
  goes to its own message.
- **An iMazing import reads a phone number or an email address
  the same way everywhere.** A sender, a chat name and a name in a group's
  member list are now read by the one rule every other import uses. A chat
  named `tel:` and a number is that number's conversation, and a group
  member listed that way is that number, not a second member by that name.
  An email address is matched whatever its capitals. A three-digit service
  number is an address. A sender named rather than numbered, such as
  `AMAZON` or `Promo2024`, is kept as that sender. Before, `Promo2024` was
  read as the number `2024`, and in a chat with a number such a message was
  shown as sent by that number.
- **An import lists every backup file it could not read, and
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
- **Staging's progress no longer jumps forward when a file the
  backup names is not there.** The byte total counted such a file's size
  and took it off only when Staging reached it, so the percentage leapt
  ahead partway through. A file that is not on disk, and an attachment
  with no bytes, are now left out of the total before Staging starts.
  Apple Messages from an encrypted iPhone backup is the one exception:
  its files are inside the backup, so one that is missing there is still
  found only when Staging reaches it.
- **Resuming an import waits while another job runs.** The
  Import screen offered to resume a paused or waiting import while an
  export or a conversion was running, and the desktop app then refused
  it. The resume button now stays off until that job ends and says which
  job it is waiting for, as the Import form does.
- **An import whose app closes or crashes keeps every Error
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
- **The server's `import` command names the file it refuses an
  attachment in.** When an attachment broke a rule, such as a path that
  leaves the export directory or bytes that do not match the fingerprint
  the file states, the `import` command stopped with the line and the rule
  but not the file. Every refusal now prints the same way: the file, then
  the line and the rule.
- **A group text from an SMS Backup+ archive stays one
  conversation when a member's contact gained an email address.** SMS
  Backup+ names a person by their email address when their contact on the
  phone had one at backup time, and by their number otherwise, so one group
  could split in two. The import now learns each address's number from that
  person's own texts in the archive and keys the group member by the number.
  A member the archive never gives a number for keeps their email address,
  and so does one whose address it gives two numbers, as a contact card
  two people share does. The run's summary counts both.
- **A received SMS Backup+ group text that doesn't name you shows
  its sender's name.** Such a text is filed under its sender, who showed up
  as a bare address. The sender now gets the name the mail gives them.
- **Logging out during an Upload waits at most 15 seconds, and
  an Upload whose session ends pauses cleanly.** Logging out during an
  Upload waited for the Upload to pause for as long as that took, and an
  Upload that did not stop kept you logged in. Logout now waits at most 15
  seconds, with a **Log out now** button, and then logs out anyway and
  says the Upload resumes from what it had sent. When your session ended
  while an Upload ran and nothing else noticed, the Upload kept going and
  recorded every remaining conversation as failed. It now pauses at once,
  records none of them as failed, and logs you out.
- **A group imported from two backups takes its current name,
  however the backups arrive.** When two copies of one group chat became
  one conversation, the name it ended with depended on whether they came
  in one upload or two: one upload kept the first copy's name, and two
  uploads kept the second's. The conversation now takes the name of the
  copy whose messages run later, which is the name the group has now. An
  older backup uploaded afterwards no longer brings back a name the group
  has dropped, and a copy with no name never clears one.
- **Working files from reading a backup no longer stay behind
  when an import is stopped, and Staging checks for room while it reads
  the backup.** Reading an encrypted iPhone backup decrypts its message
  database, and reading an SMS Backup & Restore, GO SMS Pro or SMS Backup+
  file sets its attachments aside, both as plain copies. These working
  files used to sit in the Staging Directory, and an app closed or stopped
  mid-run left them there until a later run used the same directory. They now
  sit in the Scratch Directory, are deleted when the run ends, whether it
  finished or failed, and are deleted when the app next starts if it was
  stopped. Before it writes, Staging now checks that the disk holding the
  Staging Directory and the disk holding the Scratch Directory have room, and
  stops with the space it needs while it reads the backup, not after.
- **A discarded import keeps its errors, and an import keeps its
  converted files when they cannot be read back.** Discarding an import,
  or cancelling it at a Review, recorded it with no errors, though it had
  some. It now keeps the errors it had recorded, including the
  conversations a paused Upload could not send. When Media finished but
  its files could not be read back afterwards, the import ended as failed
  and deleted the converted files. It now goes back to the form, and
  resuming it reads the files again.
- An internal fix to how an upload finishes an Import Run;
  nothing you see changes.
- **An iMazing import no longer reports a Live Photo choice as an
  error, and leaves WhatsApp chat directories' extra files alone.** When two
  photo rows named one picture, the import gave its Live Photo video to the
  first of them, as it should, but its report listed that as an error. The
  report now lists it as a note. The import also looked for Live Photo
  videos and link previews in WhatsApp chat directories, though only iMazing's
  Messages export holds them. It could attach a video beside a WhatsApp
  photo to that photo's message. It also counted the directory's other files
  as left out. It now looks for them in Messages chat directories only.
- **iMazing and Apple Messages imports name a directory they cannot
  read in full.** When an iMazing chat directory held an entry that could not
  be read, the rows whose files were in it came through with no photo or
  file, and a Live Photo video in it was dropped, without a word. On a Mac,
  a Contacts account directory that could not be read left that account's
  names out of an Apple Messages import with no message, and one Contacts
  account whose database could not be read left out every account's
  names. The iMazing import now stops and names the directory. The Apple
  Messages import now names each Contacts account it cannot read in its
  log and keeps the names from the others.
- **Minimum Video File Size takes a number of megabytes.**
  Compress & Convert added an `M` to whatever was typed in Minimum Video
  File Size, so `20MB` became `20MBM` and the run was refused over a value
  nobody typed. The field now takes a whole number of megabytes, such as
  `20`, as its label says. Anything else is refused before Staging with a
  sentence that names what was typed and asks for a number of megabytes.
  A cleared field is refused too, where before it quietly became 20.
- **A resumed Import Run follows the attachment setting Staging
  recorded.** An Import Run resumed after the app closed decided from its
  saved form whether it had a Media Stage, and its Upload took the size
  limit per file from the saved form too, not from what Staging recorded
  when it prepared the files. Nothing kept the two in agreement. A resumed
  run now reads both from what Staging recorded, so it converts or
  compresses exactly what Staging prepared for, and Upload holds each file
  to the limit the Staging Review showed.
- **An Import Run's progress no longer steps back or stops
  short during Staging.** While the Staging Stage copied attachments, two
  conversations finishing at the same moment could send their counts out of
  order or mixed together, so the attachment and conversation counts could
  move backwards and end on 3 of 4 when every one had been staged. Each
  count now follows the one before it, and both end on the full total.
- **An attachment too large after conversion says so when an
  interrupted Media Stage resumes.** When two conversations shared one
  attachment whose converted copy came out over the size limit, an Import
  Run resumed after the app closed during its Media Stage could record the
  attachment as missing in one conversation instead of too large. It now
  records it as too large, with the converted size.
- **An Apple Messages import stops when the Apple Messages
  reader stops.** The Apple Messages reader (imessage-reader) could stop
  while an encrypted iPhone backup's attachments were being copied. Every
  attachment still to come was then recorded missing, with one log line
  each. The Import Run ended looking like a run with many missing
  attachments. It now stops at that point with an error that says the
  reader stopped and why. No attachment is recorded missing because of it,
  so the import can be run again once the cause is gone.
- **A person known only by name keeps a conversation of their
  own.** SMS Backup+, iMazing and OpenExtract backups sometimes name a person
  without recording a number or address. That person's conversation could
  merge with another's: two names in a script other than Latin, such as
  "张伟" and "李娜", became one conversation, and so did "Ana Lee" and
  "Ana.Lee". A person named "AMAZON" shared the conversation of the sender
  AMAZON, and a person named "unknown" shared the conversation of sent
  messages that name nobody. Each person now keeps a conversation of their
  own. Such a person also became two contacts on import, one holding their
  name and one with no name; they are now one contact.
- **A video, photo or audio file that cannot be converted says
  why, briefly.** The error for a file the Media Stage of an Import Run
  could not convert held the converter's version, build settings and progress
  lines, with the reason at the end of several kilobytes. It now holds only
  the lines that say why the conversion failed.
- **An attachment with a blank file name is refused.** An
  import that gave an attachment a blank file name, empty or only spaces,
  stored that blank name when the server already held the file. The import
  is now refused and names the line of the file that holds it, as it
  already was when the server did not hold the file.
- **Staging's progress no longer jumps forward.** When a backup
  listed photos or files it did not hold, their sizes counted toward the
  total and came off only as Staging reached each one, so the percentage
  jumped. The total now leaves them out from the start.
- **An Upload that cannot read its staged files pauses and names
  the directory.** When the app could not read part of the files an Import Run
  had staged, the conversations it could not see were left out without a
  word, and the Upload reported success. The Upload now pauses and names
  the directory, so it can be resumed once that directory can be read.
- **A message sent twice a moment apart is shown twice.** When
  one source held a message sent twice a second or two apart, and another
  source held one copy, matching the sources hid all but one of the three.
  Now only the extra copy is hidden.
- **A photo or file imported from two places is stored once, not
  once for each.** The same picture from an Apple Messages backup and from
  an SMS Backup & Restore file used to be saved twice. It is now saved once
  for your account, and it stays as long as a message from either place
  still has it.
- **You are no longer listed among the people in your own group
  conversations.** When a backup named one of your own phone numbers or email
  addresses among a group's members, an import added you to the group as
  another person, with a contact of your own. Any address on your account now
  stays out, on every service, even one the backup did not know was yours.
- **Group texts from an SMS Backup+ archive are group
  conversations.** Every group text used to be filed as a conversation with
  one of its members alone. A group text you sent and the replies to it now
  land together in one group conversation, each reply credited to the person
  who sent it. A group text whose sender matches nobody in the group shows
  no sender instead of the first member. Each such group text is also listed
  among the import's issues.
- **A group text from an SMS Backup & Restore backup is no longer
  credited to the wrong person when the backup names no sender.** A group
  MMS without a sender address was shown as sent by whichever member the
  backup happened to list first. Such a message now shows no sender, as a
  message with no recorded sender does from any other source. A message that
  does name its sender was already credited correctly, whichever position the
  sender holds in the group.
- **An iMazing message sent in the hour the clocks spring forward
  is kept.** iMazing writes each message's date as a wall-clock time with no
  zone. A time that never showed on the clock, such as 02:30 on the March
  morning when 02:00 became 03:00, was dropped as an invalid date; it is
  now read with the offset in force just before the change, so it lands at
  the instant the new clock called 03:30. A time that showed twice on the
  November morning the clocks fall back is the earlier of the two. The
  zone an iMazing export is read in can now be given by name, such as
  `America/New_York`, as well as by offset.
- **GO SMS Pro picture messages import whole, and the ones you sent
  import at all.** An import read only the picture messages you received and,
  for most of them, mistook bytes inside the picture for phone numbers, so a
  photo from one friend could land in a group conversation with hundreds of
  made-up members. Every picture message now lands with the people who were
  actually on it, the ones you sent are included, and a voicemail notice from
  Google Voice stays in the Google Voice conversation instead of being moved
  under the caller.
- **iMazing message times are read in your account's time zone.**
  An iMazing export writes each message time without a zone, and the desktop
  app used to read them in whatever zone the computer running the import was
  set to, so the same directory gave different times on different machines. The
  import now reads them in the time zone on your profile. If the phone lived
  in another zone at the time, Processing Options on the Import screen has a
  Time zone of the messages picker for the iMazing source.
- **An iMessage you sent is always yours, however the phone
  recorded your number.** Some iPhone databases store the sending number on
  an outgoing message as `tel:+1…`, and the import kept that prefix, so those
  messages carried a sender that did not match the number on your profile.
  The prefix is now removed the same way for every message and for the list
  of addresses the backup sent from.
- **Resuming an Import Run asks again for the backup password or
  WhatsApp key.** A resumed run read the backup with an empty password and
  failed, with nowhere to type it.
- **The Import form marks every field it needs.** With **SMS
  Backup+**, **Backup Device Email Addresses** had no asterisk, and with
  **iMazing** and **OpenExtract**, **Backup path** had none, though the
  Import button stays disabled until each is filled. The Android SMS form's
  **Backup Directory** and **Backup Device Phone Numbers** lacked one too.
  All now carry the asterisk, and **Backup Directory** describes the chosen
  source's own backup files.
- **Max FPS is a ceiling.** Compress & Convert re-encoded every
  video at exactly Max FPS, so a 24 fps video came out at 30. A video at or
  below the limit now keeps its frame rate.
- **The Saved Search and Contact Group an Import Run adds work.**
  Opening the Saved Search an import added showed an error instead of that
  run's messages. Two runs from one source on the same day shared one
  Contact Group, and an import could take over a Contact Group you had made
  with the same name. Each run now gets a Contact Group of its own, and a
  Saved Search you create while an import finishes no longer costs the run
  its own.
- **One attachment no longer stops a whole import.** An
  attachment whose file name the computer refuses, such as one with a
  240-character extension, ended the Staging and nothing was imported. It
  is now recorded as missing and everything else is staged. A run with media
  turned off is no longer refused for lack of disk space it would never
  use, and a staged attachment whose name ends `.jsonl` no longer turns a
  successful Staging into a failed run.
- **Large SMS backups no longer run out of memory.** The SMS
  Backup & Restore, GO SMS Pro and SMS Backup+ readers kept every attachment
  in memory until the end of the run, so a backup of several gigabytes of
  video could get the desktop app stopped by the operating system. They now
  hold one attachment at a time.
- **An Upload interrupted at the wrong moment no longer stores
  messages twice.** When the connection dropped while the server's answer to
  a batch was arriving, the batch was sent again and messages without their
  own id were stored twice. Every message now carries an id, so a batch
  sent again for any reason stores nothing twice. A file holding a message
  without one is refused whole, naming the first lines at fault.
- **Empty attachments and lowered size limits no longer fail a
  conversation.** A 0-byte attachment was refused, and the whole conversation
  that held it failed. Lowering the attachment size limit during an Upload
  broke an attachment already uploading in parts. The limit also capped
  every request, not only attachments, so a very low limit stopped anyone
  logging in or raising it again; it now holds attachments alone. A small
  video in Compress mode is forecast at its own size, so the Staging Review
  warns when it won't fit.
- **An Import Run stops when the server can't record its stage.**
  A lost stage write left the run behind, so the next visit offered only
  Start over, or read the whole backup again, instead of showing the Review.
- **Cancel pressed just before a job starts stops it.** A Cancel
  in the moment between two steps of an import or an export was lost, and
  the next step ran to the end.
- **Two conversations with one person in one batch become one.**
  Two conversations whose addresses are the same once written the same way,
  such as `+15555550119` and `5555550119`, or an iMessage and an SMS
  conversation with one number, failed the whole batch when they arrived
  together. They now merge, as they did when they arrived apart.
- **Messages sent in the same second keep their order.** When an
  Upload split a conversation inside one second, the conversation and every
  export showed those messages out of order.
- **A message sent twice is shown twice.** When two sources each
  held a message sent twice in one second, matching the sources against
  each other hid all but one copy.
- **An import no longer matches a name to a contact in the
  Trash.** A participant the backup names without an address was bound to a
  trashed contact of that name, which failed the import or made a third
  contact beside a live one.
- **Upload errors name the real cause.** An attachment check the
  server refused said "username does not match API key" or "invalid API
  key", causes that no longer exist. It now says the account is disabled or
  may neither import nor export, or that the login was not accepted.
- **Apple Messages addresses arrive as themselves.** A phone number
  and an email address on one contact card arrived as a single address made of
  both, stored as an email identity. A received message with no sender became
  a contact called "Me". A conversation with no members lost its name, an
  unnamed group with one member left was filed as one-to-one, and your own
  address was listed among a group's participants. Each is fixed.
- **No decrypted copy of your iPhone messages is left behind.**
  Entering an encrypted backup's password made a plain copy of the Messages
  and Contacts databases in the system's temporary directory, which stayed
  there when the app or the reader was stopped. The copy now lives in the
  run's own directory and is always removed. Decrypted attachments are written
  there too, on the disk the run checks for room, so a small system
  temporary directory no longer makes every large video fail. An attachment
  that could not be decrypted is counted and listed among the Import Run's
  issues with the reason, instead of being marked missing while the run
  reported success.
- **WhatsApp imports keep what they could not find.** A message
  whose photo file was missing lost the attachment with no trace, and a later
  import with the file in place added the message a second time. It now
  keeps the attachment marked missing. WhatsApp Status no longer
  becomes a contact, and WhatsApp ids that are not phone numbers are no
  longer stored as email identities. A WhatsApp import from an iPhone backup
  no longer fails over leftovers from an earlier run in the backup directory.
- **Phone numbers from Android SMS backups keep their country.**
  The SMS Backup & Restore, GO SMS Pro and SMS Backup+ readers read every
  number by US rules, so `+6595550100` became a US number and a UK number
  matched nobody. An email address became a phone number made of its
  digits, and a message from a sender name such as `AMAZON` was dropped. Your
  own number written without its country code is now recognised as yours,
  so a received picture message is no longer filed as a group with you in
  it.
- **SMS Backup & Restore messages read back as written.** Line
  breaks in a message survive, an emoji written as a character reference
  shows as the emoji, and a broken reference costs one character instead of
  the message, or, in GO SMS Pro, the whole file. The names "null" and
  "(Unknown)" no longer name a contact, and the sender of a group picture
  message is no longer named after the whole group.
- **GO SMS Pro picture messages with newer headers import.** A
  picture message using a header from a later version of the MMS standard was
  dropped whole.
- **SMS Backup+ imports only text messages, and reads every
  directory you give it.** A call-log mail is now skipped and counted, instead
  of imported as a text holding the call's length. A backup directory that sits
  inside a directory named Duplicate, Exclude or `.git` is no longer skipped
  whole. Two people known only by names that differ outside plain English
  letters, such as "张伟" and "李娜", no longer share one conversation.
- **iMazing imports attach Live Photo videos.** A Live Photo's
  video is imported with its picture, and link previews and any other file
  no row names are counted in the report. A message no longer picks up
  another message's file whose name merely ends the same way, and a group
  known only by names no longer gets its own id as a member.
- **An attachment too large after conversion says so in every
  conversation.** When two conversations shared one attachment that came
  out of Media over the size limit, the second recorded it as missing
  instead of too large. Opening the link to an Import Run's directory not made yet
  says nothing is there yet, instead of calling it outside the Staging
  Directory when that directory is reached through a link.
- **An Apple Messages reaction belongs to the person who made
  it.** Your heart on a friend's message was stored as theirs, and theirs on
  yours as yours. Each reaction also became an extra message of its own, and
  removing a reaction showed it as added. A reaction is now stored on the
  message it reacts to, under the person who made it, and a removed one
  leaves nothing behind.
- **Two different messages alike are no longer stored as one.**
  For SMS Backup & Restore, GO SMS Pro, SMS Backup+, iMazing and
  OpenExtract, "lol" from two people in the same second of a group, or "?"
  sent twice a moment apart, was stored once and the other copy counted as
  a duplicate. Reading the same backup on a computer set to another time
  zone gave every message a new id, so importing it again stored every
  message a second time. Each message's id now takes in its sender and the
  exact instant, wherever the backup is read, and every source drops a
  repeated copy of one message in the same way.
- **The Import form's attachment choices are checked before
  Staging and hold for the whole run.** Compress & Convert with Max FPS
  cleared ran Staging to the end, hours on a large backup, then failed, and
  every resume failed the same way. It is now refused before Staging starts,
  with a sentence that names Max FPS; a Max FPS of 0 or below is refused
  too. An iMazing or OpenExtract import always copies the original
  attachments. Before, it followed whatever Attachments choice was left from
  another source, so it could re-encode videos or show "Attachments: Skip".
- **An import refused over a line that can't be read names a line
  you can find.** The server's `import` command counted only the lines that
  were not blank, so the number pointed at the wrong line. An Upload named a
  line of its own batch; it now names the staged file and the line in it.
- **An SMS Backup & Restore import counts the repeated messages
  it drops.** A message the backup held twice was kept once, as it should
  be, but the import's summary never said a copy was dropped. It now says
  how many, as it does for GO SMS Pro, SMS Backup+, iMazing and OpenExtract.
- The way a resumed Upload records the conversations an earlier
  part of the Import Run already sent was reworked, with nothing visible.
- Handling for messages without an id, which an import already
  refuses, was removed, with nothing visible.

#### Exporting and converting

- **A line of an Export's record of fetched Assets that cannot
  be read is named in the Export's log.** The Export skipped such a line
  without a word, and a line a crash cut inside a character stopped every
  later Export into that directory. Both are now skipped and named: "Line 4
  of …, the record of fetched Assets, could not be read (…), so the Export
  skips it. An Asset that line recorded is not fetched again while its file
  is in the directory."
- **The log of an Export from a server says it fetches Assets,
  and writes its warnings as sentences.** It read "Downloading 2 assets with
  8 workers (0 already downloaded)…" and "Downloaded 2 assets (22 B) and
  kept 0 already downloaded", and began each warning with "warning:". It
  now reads "Fetching 2 Assets with 8 workers (0 already on disk)…" and
  "Fetched 2 Assets (22 B) and kept 0 already on disk". A refused
  completion reads "Export Run 7 completion failed (…): …. The Export wrote
  every file all the same", and an attachment path that would leave the
  Export's directory reads "Attachment path … would leave the Export's
  directory, so the file is written at … instead". An Export run again into
  a directory where an earlier one stopped early counts the files already
  there as kept, and the size counts only what it fetched. Nothing changes
  on screen for that.
- **Two attachments whose names differ only in their extension
  both arrive whole in an Export from a server.** The Export wrote each
  attachment it fetched to a temporary file named after the attachment
  without its extension, so `menu.pdf` and `menu.jpg` shared `menu.part`.
  When both were fetched at once, one could cut the other off or mix bytes
  into it, and the Export failed or kept a damaged file. Each fetch now
  writes to a temporary file of its own, as does the copy of an attachment
  to a second path, which could also write over an attachment named
  `menu.part`.
- **When part of an Export from a server fails, the message
  names the Export Run or Asset in the words the app uses for them.** These
  messages used lowercase shorthand such as "complete export failed" and
  "asset download failed", or a bare web address when the connection failed,
  and a completion the server refused named the run twice. Now each one
  names what was being asked for once, such as "Export Run 7 completion
  failed" or "Asset 3f2b… fetch failed", followed by what went wrong.
- **An Export from a server no longer calls itself a backup in
  the log.** It began with "Backup query: from:sam" and, when the directory
  held a finished run, "Previous backup completed successfully". A backup is
  the phone's file an import reads, so the log now says "Exporting the
  messages that match: from:sam" and "The previous Export Run finished.
  Checking for new messages…". The line with the run's counts names its
  record: "Export Run 7 holds 3 messages in 1 conversation", where it said
  "Export 7". In a directory an earlier version exported into, the first
  Export from each account does not say the previous Export Run finished,
  because the note that run left in the directory is now written
  differently. It still skips every attachment already there.
- **An EML or mbox export of a message whose id or address holds
  a line break converts whole.** A line break in a message's id or in the
  phone number or address it was sent from or to ended the mail's headers
  early, so converting the file lost the message's details or refused the
  whole file. The same went for an attachment whose file type held one. Such
  a value is now written so the mail stays whole, and converting the file
  gives it back exactly as it was exported.
- **An EML export of a message whose id holds a slash, a line
  break or a character Windows does not allow in a file name finishes.**
  Each message's file is named partly after its id. A slash in the id made
  the export of the whole conversation fail everywhere, and a line break or
  a character such as `:` or `?` made it fail on Windows. Those characters
  are now written as a `%` and two hex digits. A `%` in the same part of the
  id is written that way too. Every file name now works on Linux, macOS and
  Windows. Messages whose ids hold none of those characters keep the file
  names they had.
- **The rest of the log says each count in plain words too.**
  The lines around a run's summary still wrote counts as "1 file(s)" or
  "3 conversion(s)": converting attachments, an Export from a server, and
  the server's own commands. They now say "1 file" and "3 conversions",
  and the words around a count agree with it, such as "1 conversion failed.
  That original stays without a Thumbnail". An SMS Backup+ run with
  verbose logging no longer ends with two lines of raw counts, because its
  summary already gives each of those counts in words.
- **An EML or mbox export keeps every space of a message's
  details.** A run of spaces in a message's details, such as a name, a
  transcription, an earlier version or a detail from the source app, could
  come back as one space when the file was converted, and a space that
  opened or closed one of them was lost. Such a detail is now written so
  that converting the file gives it back exactly as it was exported.
- **Converting writes each attachment once.** With Media set to
  Convert or Compress, Convert copied the export's whole attachments
  directory, converted the copy, and then wrote every attachment again from
  the export. The copy stayed in the new output beside the files the
  conversation named, so the output could hold two of each attachment on a
  disk checked for room for one. Converting an SMS Backup & Restore backup
  also copied any attachments directory beside it, though the backup holds
  its own. Convert now writes only the files the conversation names, and
  the check for room counts what the run writes.
- **Converting an EML or mbox file refuses a damaged message
  instead of quietly dropping what it could not read.** A message whose
  attachment details, app message, message parts or details from the source
  app could not be read was converted as if it had none, so a conversion
  could lose every attachment's name and type without saying so. Such a
  message now stops the conversion with a message that names what could not
  be read and says to export the backup again, as a damaged list of
  participants, reactions or earlier versions already did.
- **Exporting to CSV, and reading an EML or mbox file back,
  type a phone number written with `tel:` as a phone number.** The
  `identity_type` column of a CSV export, and a participant read back from
  an EML or mbox file, now follow the one rule every import uses. Before,
  `tel:+15555550157` was typed `other` there and `phone` everywhere else,
  so the same person could arrive as two identities.
- **Converting no longer asks for room for an attachment whose
  file is missing.** Convert checks the disk for room before it starts,
  and that check counted the size an attachment's record gave even when
  the file was not in the export being converted, so a conversion could
  be refused for space it would never use. An attachment with no file is
  now left out of the check, and a refused conversion still leaves the
  earlier output as it was.
- **Exporting Apple Messages to EML or MBOX with attachments
  embedded no longer asks for room for a file that is gone.** From a Mac,
  or from an iPhone backup that is not encrypted, the check for room
  counted an attachment whose file was missing at the size Messages
  recorded. It now counts nothing for it, and the log names the file. In
  an encrypted iPhone backup the files are inside the backup, so each one
  is still counted at its recorded size.
- **Converting, and exporting Apple Messages to a format other
  than JSON Lines, no longer count an attachment with no file in the byte
  total.** The byte total in the progress and in Convert's log counted
  the size such an attachment's record gave and took it off only when the
  run reached it, so the total dropped partway through. Every attachment
  known to have no file is now left out of it before the run starts, as
  Staging already does. The Apple Messages check for room for the
  attachment files it writes leaves it out too. Convert's log also names
  each file it found missing, as Staging's does.
- How an exporter writing a format other than JSON Lines counts
  the size of an attachment with no file was reworked to match Staging,
  with nothing visible.
- **Converting an SMS Backup & Restore backup says what it
  left out.** The log said nothing about the repeated copies it dropped,
  the messages with an invalid date, a date outside the range, no usable
  address or an unknown type, the drafts, the picture messages with nobody
  on them, the message parts it could not read, or the character codes
  that stand for no character, and it named
  only the first five files it could not read. It now lists each count
  as soon as the backup is read, even when the conversion then stops, and
  names every file it could not read.
- **Exporting from a second server or account no longer makes
  the first download every attachment again.** When Export from two
  servers, or two accounts, wrote into one directory, the run that
  finished last forgot which attachments the other had already
  downloaded, so the other's next Export downloaded all of them again. Each
  server and account now keeps its own record.
- **Nothing an export did not write is ever removed.** Every
  step that removes or replaces files from an earlier export, including
  the obfuscated export's placeholders, now checks for itself that an
  export wrote the directory, and refuses one that it did not. Before,
  three of those steps relied on the step before them to check.
- **Messages sent to nobody survive an export as SMS Backup+
  mail.** OpenExtract keeps sent texts that name no recipient in one
  conversation. Exported as **EML (SMS Backup+)** and imported again, they
  came back as a conversation with a made-up person, who was then added
  to Contacts. They now come back as the same conversation with no one in
  it.
- **A person known only by a name that looks like a number keeps
  their messages through an SMS Backup+ export.** A person a backup named
  "+1 555 0101", with no address, lost every message when exported as
  **EML (SMS Backup+)** and imported again. The import now keeps them in
  that person's conversation.
- **Every format checks for room before it writes.** Only an
  import to the server used to check for free disk space; writing CSV,
  JSON, EML, MBOX or SMS Backup & Restore XML, and **Convert** in
  Settings, failed part-way with a write error when the disk filled. They
  now check first and stop with the space they need. **Convert** from an
  SMS Backup & Restore backup also sets the backup's attachments aside in
  the Scratch Directory, not in the directory it writes to.
- **A conversion can no longer start in the middle of an Import
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
- **A group text exported to SMS Backup & Restore names the
  people in it, not one sender.** An export to SMS Backup & Restore put the
  name of whoever sent each group message where the app keeps the names of
  everyone in the group, so the file held a name the app would not write
  there, and reading it back lost that name anyway. Each group message now
  carries the names of the people in the group, as SMS Backup & Restore
  writes them. One-to-one messages are unchanged.
- **An export stops when it cannot read an earlier export's
  email directory.** An export or conversion written into a directory an earlier
  one used first removes the earlier one's email conversation directories. A
  conversation directory holding an entry that could not be read could look
  as if it held no email, and then stayed beside the new export. The
  export now stops and names the directory.
- **An obfuscated export keeps a reply to a message in another
  conversation.** Apple Messages can reply or react to a message in another
  conversation. In an obfuscated export, that reply or reaction pointed at
  no message. It now points at its message, wherever in the export that
  message is. A message the export leaves out, such as one outside the date
  range, still can't be pointed at.
- **GO SMS Pro and SMS Backup+ attachments keep their size when
  their files are left out.** A run with Attachments set to Skip wrote each
  GO SMS Pro and SMS Backup+ attachment without its size. Each one now
  carries its size, as SMS Backup & Restore attachments already did.
- **An obfuscated export no longer records the real size of each
  photo or file.** Obfuscate replaces every attachment with a placeholder,
  but the attachment kept the size of the real file, which can be enough
  to recognise it. An obfuscated attachment now carries no size.
- **Convert keeps the previous output when an Android XML backup
  can't be read.** Converting a broken `smses.xml` into a directory an earlier
  conversion wrote removed that conversion's files before the backup was
  read, so the run failed with nothing left. The backup is now read first,
  as every other input is, and a backup Convert can't read, or one with no
  conversations in it, stops the run with the previous output left as it
  was.
- **An obfuscated export leaves out attachments in subdirectories
  too.** A real photo or file inside a subdirectory of the export's
  attachments stayed in the export that exists to leave it out. A shortcut
  (symbolic link) there is now removed, and the file or directory it points
  to is left untouched.
- **Android XML holds only SMS and MMS.** Export and Convert
  wrote every message as a text message, so an iMessage or a WhatsApp
  message came back from a re-import as an SMS. They now leave every other
  message out, and the log says how many were left out and why.
- **An export from the Conversations list holds those
  conversations.** Export opened from a filtered Conversations list wrote
  only the matching messages, or refused a search such as `messages:>100`.
  It now writes every message of the conversations the list showed.
- **Converting to CSV, EML or MBOX and back loses nothing the
  server reads.** Each message keeps whose number it was sent from, an
  attachment keeps its size and the reason it is missing, and a text or HTML
  attachment comes back with its own bytes instead of the next attachment's.
- **An export stays inside its directory.** An attachment is never
  written outside the output directory, whatever path it was stored under. An
  export, or Convert, whose output directory is the backup or a directory above it
  is refused before anything is written. An obfuscated export that can't
  remove a real attachment fails and names the file, instead of reporting
  success with the file still there.
- **Exported attachments are checked.** A download that answers
  with something other than the attachment, such as a sign-in page from a
  proxy, is refused, instead of being saved as the attachment and skipped
  by every later export into the same directory.
- **Export and Convert name the directory they wrote to.** The
  success message followed whatever the form showed afterwards, and the
  directory could be changed while the job ran.
- **Convert names a file from an older format.** A file in the
  version-3 format is refused by name, where a directory of them failed with
  a message that said nothing useful and a directory mixing them with current
  files left those conversations out without a word. A refused file now
  stops the run before the previous output is removed.

#### Search

- **An edited message imported from two backups shows its
  earlier versions.** The same iPhone imported through iMazing and through
  Apple Messages gives two copies of each message, and the Message Crate
  shows one of them. When it showed the iMazing copy, which records no
  edits, the message listed no earlier versions, and a word only an earlier
  version held found nothing. The copy shown now lists the earlier versions
  of the hidden copy that holds them, search finds it by them, and Export
  writes them.
- **A conversation opened from a Message Tag page keeps the
  search box as it was.** Opening a conversation from a Message Tag page put
  `tag:Holiday` into the search box, as though it had been typed, and the
  Messages list then showed every message with that tag instead of asking
  for a search. The tag now stays out of the box: the box shows only what
  was typed, and the lists are the same in the conversation as on the tag
  page.
- **Searching for part of a group conversation's id no longer
  lists every group conversation.** Each source gives its group
  conversations ids of one shape, such as `group:…`, `chat-…` or `…@g.us`.
  So typing `group`, `chat` or `g.us` on Conversations listed every group
  conversation from that source, whatever its title. `in:`, `with:` and
  `identity:` found them the same way. A group conversation is now found by
  its title and by the people in it.
- **Searching Conversations for `name` no longer lists every
  conversation known only by a name.** Typing `name` on Conversations listed
  every conversation whose backup gave a name and no address, and `less` the
  conversation that names nobody; `in:nam` on Messages listed every message in
  them. Each is now found by the name of the person in it or by its title,
  with plain text on Conversations and with `in:` on Messages.
- **`identity:` no longer lists every conversation known only
  by a name.** `identity:nam` listed every conversation whose backup gave a
  name and no address, and `identity:less` the conversation that names
  nobody. Neither search lists them now, unless someone in them has an
  identity that matches.
- **Import, Export and Settings show no search box.** The search
  at the top searches the list of the section you are in, and these screens
  have no list yet, so the box there searched nothing. On Export, typing in it
  changed which conversations Export would export. The box is gone
  from these three screens and comes back when you return to a list.
- **A search pasted and run at once is the search that runs.**
  Pasting a search and pressing Enter straight away searched for nothing.
  Typing very fast lost letters, and the search ran on the last letter
  typed. The box now keeps everything you put in it, and Enter runs it.
- **Excluding something from a search no longer hides the rows
  that have nothing to compare.** A search with `-` in front of a word left
  out every row with no value for that word, so those rows appeared under
  neither the word nor its negation. `-import:last` found no messages at all
  before the first import, and a negated date word on Contacts left out
  every contact with no messages. A search and its negation now always
  divide the list between them.
- **Searching for a word with punctuation in it works.** A
  search such as `a&b`, `o'bri*`, or text pasted with a hidden NUL
  character failed with an error. Punctuation inside a word now always
  means the words next to each other in that order, and a NUL is read as
  a space.
- **The Trash stays out of a search everywhere the search looks.**
  A contact whose only group conversation was in the Trash still matched
  `kind:group`, a conversation whose only Family member was in the Trash still
  matched `group:Family`, and `conversations:` counted trashed conversations.
  A search now leaves the Trash out on both sides until it uses `trashed:`,
  and then the Trash counts on both sides, which is what searching the Trash
  screen already did. The contact list's Last heard from date and its ordering
  leave trashed conversations out too, so they agree with `last-message:`.
- **Search forgets a deleted message's attachment name.** After
  a message was deleted, a search for its attachment's file name found the
  next message imported.
- **The search box sends the search you typed.** Picking a
  suggestion after a `-` dropped the minus and reversed the search. A Contact
  Group or Message Tag whose name holds a comma, a leading `#` or a trailing
  `*` is quoted. "Between" with only an end date includes that day. Text
  typed on Trash or a tag page stays inside that page, so `or` no longer
  brings in conversations from outside it. A second Enter runs the text the
  box shows.
- **Searches that failed now answer.** A date the account's time
  zone skipped, such as 30 December 2011 in Samoa, dropped the connection. A
  date beyond year 9999 compared the wrong way round. A long comma list
  failed with a server error instead of being refused as too complex. An
  empty quoted phrase, `""`, matched everything; it is now refused.
- **Search finds Greek and Turkish names.** `name:ΚΩΣ*` now
  finds "ΚΩΣΤΑΣ", and `name:istanbul` finds "İstanbul Office".
- **An attachment added to a stored message is searchable.** An
  import that added a missing attachment to a message already in Message
  Crate left its name out of search.
- **`service:` on Contacts reads the whole conversation.** A
  contact you texted over SMS who never replied is now found by
  `service:sms`.
- **Each account sees only its own recent searches.** The next
  account to log in on the same browser was offered the last one's.
- **Searches and filters stay put.** Opening a conversation from a
  searched or filtered list kept the list filtered only until the first
  click. A Saved Search click no longer rewrites the Contacts or Trash search
  you go Back to. Trash names an unknown search word once, and says when
  its requests fail instead of saying "Trash is empty."
- **`source:` names every source an import reads.** It took only
  `imessage`, `whatsapp` and `sms`, where `sms` meant SMS Backup & Restore,
  so messages from iMazing, OpenExtract, GO SMS Pro and SMS Backup+ could not
  be searched by source. It now takes `imessage`, `whatsapp`,
  `sms-backup-restore`, `imazing`, `openextract`, `go-sms-pro` and
  `sms-backup-plus`, and the Advanced Search form on Messages has a Source
  field listing them by name. On Conversations, `source:` also finds a
  conversation that source holds only as duplicates.
- **A conversation of only duplicates shows no date.** Under an
  `import:` search, such a conversation read as last active on 1 January
  1970.

#### Contacts and identities

- **Your profile lists a phone number once when it is both a
  Text Message and a WhatsApp identity.** The account's phone numbers named
  such a number twice, with no service, so the Android SMS owner numbers an
  import offers listed it twice too. Each number now comes once, naming every
  service it is your identity under.
- **An import names a nameless contact, whatever made it.** A
  contact with a number and no name stayed Unknown after an import that knew
  the number's name, when an Address Book load or you had made it rather
  than an earlier import. An import now fills in any contact's missing name.
  It still never changes a name a contact already has.
- **A misspelt service no longer puts an identity on Text
  Message.** Adding, swapping or removing a contact's identity, or one of your
  own, takes Text Message or WhatsApp and nothing else. Any other service used
  to be read as Text Message without a word, so a WhatsApp number with a typo
  in its service landed on Text Message; now the server refuses it and says
  which two it takes. An email address is added on Text Message, where
  iMessage reaches it, as the app already does for you.
- **One number is one identity however it arrives.** A number
  written with `tel:` in front, in a backup that gave no type for it, became
  a separate identity from the same number as a message sender, on a contact
  of its own. An email address added to a contact under iMessage, or to
  your own identities under Phone, was saved as a phone number. Every
  address is now typed by what it is, whatever service it came over or was
  added under.
- **Changing a contact's identity can move it to another
  service.** Changing a WhatsApp number to a Text Message number in one edit
  was refused with "previous address not found on contact". The old
  identity is now found on its own service. Changing a WhatsApp number to an
  email address without naming a service saved an email address on
  WhatsApp; it is now refused with the reason, since WhatsApp carries no
  email addresses.
- **An Address Book loaded straight back renames nobody.** A
  name cell that started with a tab, written `'` then a tab in the
  spreadsheet, created a contact whose name kept the tab, and loading the
  exported file back renamed that contact without it and counted it as
  updated. A contact's name is now saved without spaces, tabs or line breaks
  at its start or end, so the file loads back with nothing changed.
- **The Contact Groups and Message Tags menus say why a name is
  refused.** Creating a Contact Group from the Contact Groups menu on the
  contacts list, or a Message Tag from the Message Tags menu on the
  conversation list, did nothing when the server refused the name, such as a
  Contact Group name holding `;` or a name over 80 characters. The menu now
  keeps the typed name and shows the reason, as the sidebar already did.
- **A screen reader says which contact is open.** The open
  contact in the Contacts list was shown only by its highlight, so a screen
  reader gave no sign of which one was open. The open contact is now
  announced as the current one, in the browser and in the desktop app, and
  one click still opens a contact. In the desktop app the highlight also
  moves to a newly opened or checked contact, where before it could stay
  where it was first drawn.
- **Adding a WhatsApp identity checks that it was added.** When
  a number was already a Text Message identity, adding it on WhatsApp
  closed the dialog even if the server added nothing. The dialog now stays
  open and says "The server did not add that identity." When the list of
  identities cannot be loaded again to check an add or a removal, the
  dialog says so and asks you to try again.
- **International phone numbers keep their country.** A number
  written with a country code, such as `+65 9555 0100` in an address book or
  `+44 7700 900123` as your own number, is now matched as that number. Before,
  some were read as a US number with the same digits and named the wrong
  person, and some matched nobody.
- **A contact's identities and selected contacts count what the
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
- **Unknown and No group list the right contacts.** Once the
  whole Contacts list had loaded, Unknown listed every contact and No group
  included Unknown ones.
- **The Demo Account's email is one of its identities.** My
  Identities showed the email until the list loaded, then dropped it.
- **Contact Group and Message Tag names stay usable.** "Unknown"
  and "none" can no longer be taken by a Contact Group, nor "none" by a
  Message Tag, and a Contact Group name can't hold `;`, which the Address
  Book separates names with. A link to a Contact Group or Message Tag opens
  that one, whatever characters its name holds, and a link to one that no
  longer exists says so instead of listing everything. The contact tables
  use the same column names.
- **The Address Book load reads a file the way you meant it.** A
  number whose `+` a spreadsheet dropped keeps the identity the contact
  already has, instead of becoming a new US number. A row with a stray extra
  comma refuses the load instead of shifting every cell after it. A refusal
  over a contact in the Trash says to restore it or delete it for good.
  Export writes a `'` before any cell that starts with `=`, `+`, `-` or `@`,
  so a spreadsheet shows a contact named `=HYPERLINK(…)` as text instead of
  running it as a formula, and keeps the `+` of every phone number. The load
  takes that `'` off again, whether or not the spreadsheet kept it.
- **A conversation no longer names a contact in the Trash.** A
  participant whose contact was trashed showed that contact's name and
  linked to a contact that could not be opened. A group with no recorded
  members no longer lists its own id as a member.
- **Identities count and date the way the rest of the app does.**
  A conversation two identities of one contact share counts once in the
  contact's total. First and last dates are days in your account's time
  zone, not UTC.
- **WhatsApp identities stay WhatsApp.** Removing a WhatsApp
  identity in Settings removed the Text message identity with the same
  number, and the WhatsApp one could not be removed at all. Changing a
  WhatsApp contact's number moved it onto Text message. Profile Setup showed
  a number on both services as two Text message rows and would not continue.
- **A contact that fails to load says so.** The contact drawer
  and the selected-contacts figures offer Try again, instead of "Loading…"
  for good or a row of dashes.
- **A number's messages stay with its contact, whatever service
  carried them.** A message from a contact's number over a service Message
  Crate doesn't know, such as an iPhone message sent by satellite, was filed
  under a new contact with no name and left out of the named contact's
  counts.
- **The contact drawer opened from a conversation covers the
  column resize handles.** It sat below the handles, so a handle could show
  through it.
- **The contact drawer opened from a conversation stays inside
  the window.** In a window narrower than the list plus the drawer, its right
  side ran past the window's edge and was cut off. It now stays inside the
  window, and in a window as narrow as a phone it covers the list instead.
- **The contact drawer opened from a conversation lines up with a
  list that appears after it.** When the list showed up only once the drawer
  was already open, the drawer stayed against the right edge of the window,
  as if there were no list, until the window was resized. It now moves to the
  list's edge as soon as the list appears, and back when the list goes.
- **The Contact Identity card and the selected-contacts card fit
  inside their panel.** The Contact Identity card, and the card that sums up
  the contacts you selected, were a little wider than the panel they sit in,
  so the panel scrolled sideways.

#### Accounts, Settings and screens

- **The desktop app reaches an HTTPS server whose certificate
  your computer trusts.** A Message Crate behind a reverse proxy whose
  certificate comes from a private certificate authority, such as one made
  by mkcert or Caddy's internal authority, opened in the desktop app's
  window, but an Upload, an Export from the server and an attachment
  download failed with a certificate error. The app checked those against the
  public authorities it carries and nothing else. It now also trusts the
  authorities the operating system trusts, as the window does. A certificate
  that neither trusts is still refused.
- **The desktop app saves a large attachment without holding it
  in memory.** Downloading a video of several hundred megabytes in the
  desktop app loaded the whole file into memory two or three times over
  before saving it, which could use more than a gigabyte and fail on a
  smaller computer. The app now asks where to save first, then writes the
  file to that place as it arrives from the server, so memory stays the same
  whatever the size of the file. A download that breaks off part-way leaves
  any file already at that place as it was.
- **The Import, Export and Convert screens name running work as
  the desktop app does.** While an Export or a Convert was running, the
  screens said "An export is running." or "A conversion in Settings is
  running.", where the desktop app's own refusal says "An Export is running."
  and "A Convert is running." The screens now use those names too, so each
  piece of work has one name wherever you meet it.
- **The desktop app names what is running in the words its
  screens use.** When you started an Import Run, an Export or a Convert while
  another was running, the app could say "Another job is running: an
  extract", naming the work with words that appear nowhere else. It now names
  both the running work and what you started the way the screens do, as in
  "An Export is running. Staging can start once it ends."
- **The desktop app names the work a bug in Message Crate
  ended.** When Staging, the Media Stage, the Upload, an Export or a Convert
  ended because of a bug in Message Crate, the app said "The job stopped
  because of a bug in Message Crate.", naming the work with a word that
  appears nowhere else. It now names it the way the screens do, as in
  "Staging failed because of a bug in Message Crate." The Upload says it
  paused, since its run keeps everything staged and can be resumed.
- **On the light theme, the contact drawer's shadow falls on the
  list it covers.** The drawer opens from the right, and its shadow fell to
  the right, under the drawer itself, so the drawer's left edge had no
  shadow. It now falls to the left, as on the dark theme and as the Sources
  drawer's does.
- **Panels, menus and drawers stand out on the dark theme.**
  Their shadows were tuned for the light theme and all but vanished on the
  dark theme's dark surfaces. The dark theme now has its own, darker
  shadows, and the drawer that slides in from the right has a thin light
  edge.
- **Settings fits a phone-width window.** In a window as narrow as
  a phone, Settings scrolled sideways, because its row of tabs was wider than
  the page and the navigation panel kept its full width. The tabs now wrap
  onto more lines, the navigation panel takes at most half the window, and the
  System and Appearance settings and the Storage page buttons stack to fit.
  Wider windows look as before.
- **A focused button shows no white line in the dark theme.**
  The keyboard focus ring on buttons, the contact drawer's close button,
  the date field's calendar buttons and the import form's section headings
  had a thin white line between the button and the ring. The gap now shows
  the colour behind the button, in every theme.
- **The focus ring in an import's results and the contact
  drawer sits 1 pixel from the edge.** The sections of an import's results
  and the conversation counts in the contact drawer drew their keyboard
  focus ring 2 pixels out, further than most buttons draw theirs.
- **Every button and tab draws its focus ring 1 pixel from its
  edge.** The search box's clear button and its Clear all, the sort button,
  the account menu, the phone numbers in a phone field, the theme choices in
  Appearance, the tabs in Settings and on the login card, the buttons above a
  conversation, the Import history dates and the expand buttons in an
  import's errors and notes drew their keyboard focus ring flush against the
  edge, unlike every other button. They now leave the same 1-pixel gap. The
  day the date picker's keyboard cursor is on shows a 2-pixel ring inside it,
  the same as a focused table row, where it showed a 1-pixel one.
- **A Settings tab that can't be opened yet shows the not-allowed
  pointer.** While the owner adds an account, Profile, Storage and Audit
  Trail are greyed out until the account exists, but the pointer over them
  was the plain arrow. It is now the not-allowed pointer every other control
  that is turned off shows.
- **The panel resize grips move by exactly 8 pixels.** Each
  arrow key on the grip of the left panel or the list column moved the
  panel 9 pixels wider or 7 narrower, and pressing the grip without moving
  it widened the panel by 1 pixel. Arrow keys now move the panel 8 pixels,
  or 24 with Shift. Pressing the grip leaves the width as it was. A screen
  reader hears the panel's own width.
- **Opening an Import Run in Settings → Storage → Import
  history keeps the list inside the page.** Opening any Import Run made the
  Import history table about a million pixels wide. Every column but Date
  sat far off to the side. Opening one that recorded errors or skipped
  items made the table keep getting wider while it stayed open, and its
  Import Errors and Notes tables showed only their first column, so no
  error or note could be read. The run's details now open below its row at
  the table's own width, and its errors, skipped items and notes show every
  column and scroll inside their own box.
- **Import history in Settings → Storage loads quickly however
  many problems your imports recorded.** The list used to bring every error
  and skipped item of every import on the page, so a few large WhatsApp
  imports with thousands of skipped files each could make it slow to open.
  It now shows how many each import recorded, in a new Issues column, and
  reads the problems themselves only when you open that import.
- **Deleting your account in the desktop app deletes its
  Staging Directories on this computer.** Deleting your own account during
  or after an import left that import's Staging Directory on disk, with
  nothing to offer it again. The delete dialog now names the account's
  Staging Directories on this computer, deleting the account deletes them,
  and one that cannot be deleted is named afterwards so you can remove it
  by hand.
- **An expired session says to log in again.** When your
  session had expired, or was ended from another window, an Upload or an
  Export said "invalid API key", though the app sends no API key. It now
  says the server did not accept the session, and to log in again.
- **Two accounts can each have the same email address.** When a
  second account added an email address that another account already had,
  the address was linked as its identity but left off its profile. Each
  account now lists every email address it holds.
- **Changing a password checks things in a sensible order and
  says so in full sentences.** Message Crate now checks the current password
  first, then that the new password was typed the same way twice, then that
  it differs from the current one, and tells you only the first thing that
  went wrong. The messages read as sentences ("Current password is
  incorrect.") and the Change password button no longer sits tight against
  the last field.
- **Attachments show for an account without Export.** An account
  whose Export permission was off saw no photos, videos or audio in its own
  conversations.
- **A refused save says why.** Renaming a contact, and creating,
  editing or deleting a Saved Search, now show the server's reason and keep
  the form open. Deleting a Message Tag or Contact Group asks first.
- **Profile Setup and Copy work over plain HTTP.** In a browser
  reaching a Message Crate on another machine without HTTPS, Profile Setup
  stopped with an error, and Copy buttons did nothing.
- **Settings → Storage shows every run as it is.** Import and
  Export history page through every run, not only the newest 40. A cancelled
  run reads as cancelled, not failed, and "Cancelled" is spelt one way on
  every screen and in the user guide. A run that has not finished no longer
  shows its start time as its finish. The Import badge goes as soon as a
  waiting run is discarded.
- **The API Tokens section says what a token can do.** It
  promised that a token could delete messages, which no token can, and now
  shows when each token expires. The secret of a new token stays on screen
  until you close its dialog, through a stray click, Escape, or leaving
  Settings before it arrives. A token shows only the permissions its account
  holds: one made by an account that may not import no longer says it can,
  and a permission the Owner turns off shows as off on every token. A token
  made while its account lacked a permission keeps it off after the Owner
  turns it on, so make a new token then.
- **Delete account asks for what it needs.** An account with no
  password confirms with its username alone, the dialog says username, a
  refusal such as a wrong password shows inside the dialog, and the typed
  password is cleared when it closes.
- **Permissions and passwords hold.** An account the Owner does
  not allow to delete can no longer delete itself, and with it every
  message; Settings says to ask the Owner. Password guesses count per
  account however the username is capitalised, and wrong current passwords
  on a password change or account deletion count too. The username `demo`
  stays the Demo Account's after it is deleted, so it can always be added
  back. A deleted account's number is never given to a new one.
- **A username counts characters, not bytes.** A 70-letter
  Cyrillic username is accepted.
- **Every change shows on every screen.** After a password change,
  a rename, deleting messages, or emptying the Trash, other screens showed
  the old state for up to 30 seconds or until the window was focused. A
  password change now also says the account's API Tokens were revoked.
- **An ended session goes to the login screen.** After a session
  expired or ended in another tab, every screen showed an error. A profile
  that fails to load now says so with a retry, instead of opening screens
  the account should not see.
- **Lists show every row once, and one click opens it.** A list
  loaded page by page no longer repeats or skips a row when something
  changes between pages. In the desktop app, one click opens a contact in a
  search result or a list sorted by Last heard, the range shows at once, and a
  first page that fits the window still loads the next.
- **An action stays with its conversation or contact.** A banner,
  an error or a pending Move to trash on one conversation or contact no
  longer shows on, or closes, the next one you open.
- **Settings fields keep what you typed and show what is in
  use.** A display name typed but not saved survives a time zone or identity
  change. The time zone field stores the zone you picked, not another one
  with the same rules today. A Staging Directory that can't be used says why,
  and the attachment size limit no longer offers to save a rounded value.
  Emptying the display name and saving clears it, on your own account and
  when the Owner clears another's; before, the old name came back.
- **The connection screen keeps track of where it is.** Applying
  an address that does not answer says so and says whether you are still
  connected. In the desktop app, "Use the Message Crate on this computer"
  starts the app's own one.
- **The desktop app follows its Message Crate.** The app's own
  server listens where "Let other devices on this network connect" says,
  and changing the box no longer restarts it during an import or starts a
  second Message Crate while the app uses another one. When a Message Crate
  the app found stops, the app notices and starts its own. A slow Message
  Crate is no longer mistaken for another program on the port.
- **The website opens with browser storage blocked.** It stayed
  blank; it now opens with the default theme.
- **Small fixes in conversations and dialogs.** A video with no
  stored file shows a file chip instead of nothing. The Sources panel shows
  each share beside the count it measures. A Contact Group or Message Tag
  dialog can't be dismissed while it saves, a second click closes the sort
  menu, the list column's resize handle moves from the width you see, and
  Browse says when the file dialog can't open.
- **Delete all messages finishes whole and spares a running
  import.** A failure part-way left the conversations deleted and the rest
  in place. While an Import Run was uploading, the delete removed the
  attachment files of the run's next batch, and that batch stored its
  messages without their attachments. The delete now succeeds or fails as
  one step, waits for a batch in progress, and leaves the attachment files
  on disk while the account has an Import Run going; they are removed by
  the next Delete all messages with no import running, or with the account.
- **The Sources panel dims the screen the way every other dialog
  does.** The shade behind it now follows the light or the dark theme instead
  of one fixed grey.

#### The server

- **The server's progress lines and its log read as
  sentences.** The server's output during an import, a Demo Account build
  and `process-assets` started lines with labels such as "sql:", "dedupe:",
  "import:", "db:" and "[dry-run]", and the Demo Account build started its
  steps with "Reset demo —". Warnings in the server's log started in lower
  case. Each now says what happened, for example:
  - "sql:      promote: chunk 1: 5 messages inserted, 5 of 9 so far" is now
    "Batch 1 wrote 5 messages, 5 of 9 so far".
  - "dedupe:   pass A exact content_key…" is now "Hiding exact duplicates,
    the messages that share a content key…".
  - "import:   [2/5] chat.jsonl: …" is now "Read chat.jsonl, file 2 of 5.
    So far: …".
  - "[dry-run] would remove …" is now "This dry run would remove …", and
    "account 1: assets=…" is now "Processing account 1's Assets in …".
  - "Reset demo — generating the medium data set" is now "Generating the
    medium Demo Data set", and "Demo reset complete" is now "The Demo
    Account is rebuilt".
  - "database schema differs from this server's; rebuilding empty (re-import
    your data)" is now "The database's schema differs from this server's, so
    the database is rebuilt empty and its messages must be imported again".
- **The server and the demo seed write their last warnings as
  sentences.** Lines on standard error started with "warning:", "stopping:"
  or "skip —". Each now says what happened, for example:
  - "warning: could not add the Demo Account: …" is now "The Demo Account
    could not be added: …", and the line after it starts with a capital.
  - "warning: the Demo Account has 1 original whose Preview or Thumbnail
    could not be made. reset-demo continues" is now "The Demo Account has 1
    original whose Preview or Thumbnail could not be made. Its build goes on
    all the same", since a new database and Owner Home build the Demo
    Account too.
  - "warning: could not write server.ready back: …" is now "server.ready
    could not be written back, so sqlite-web goes on waiting: …".
  - "warning: installed the generated demo bundle but could not remove
    backup …" is now "The newly generated demo files are in place, but the
    backup of the previous ones at … could not be removed: …".
  - "shutting down" is now "The server is shutting down", and
    `process-assets` says "process-assets is stopping." when it is stopped.
- **`process-assets` says what it did to an incomplete original
  and a damaged or shared Preview or Thumbnail.** A transfer that never
  finished leaves an incomplete original. When one could not be removed,
  `process-assets` ended with "1 original whose Preview or Thumbnail could
  not be made", although nothing was being made. A removed incomplete
  original, a dropped damaged Preview or Thumbnail, and an existing one given
  to more attachments counted in "left 1 original as it was". The line that
  ends the run now adds "removed 1 incomplete original", "dropped 1 damaged
  Preview or Thumbnail" and "shared 1 existing Preview or Thumbnail with more
  attachments". On a failure it adds "1 incomplete original that could not be
  removed" and "1 damaged Preview or Thumbnail that could not be dropped".
  Each is there only when it happened. The run ends with an error naming
  each failure. The `reset-demo` summary has a line for each count, and its
  warning now reads "the Demo Account has 1 original whose Preview or
  Thumbnail could not be made. reset-demo continues".
- **`process-assets` and `reset-demo` no longer call a failed
  Thumbnail a failed conversion.** When the Preview or Thumbnail of an
  original could not be made, `process-assets` ended with "1 conversion
  failed. That original stays without a Thumbnail or a browser preview",
  also when only the Thumbnail failed, and `reset-demo` warned "1 demo
  attachment failed conversion". They now say "1 original whose Preview
  or Thumbnail could not be made", in the error and in the warning. The
  `reset-demo` summary's "Browser previews" section, with its "converted
  for web", "left as-is" and "conversion failures" lines, is now "Previews
  and Thumbnails", with "Previews made", "Thumbnails made", "left as they
  were" and "not made". It also gives the count of Thumbnails, which it
  left out. Stopping `process-assets` says it stops the Preview or
  Thumbnail being made.
- **The Session always names its username.** `GET /v1/session`
  described `username` as possibly empty, so every program reading it had to
  allow for a Session with no username. It always carries one now. An
  account deleted at the moment its Session is read answers
  `401 Unauthorized`, as a credential naming no account does, instead of a
  Session without a username.
- **The HTTP API reference describes every field.** 59 fields,
  among them the Import Run's mode and source, an upload's part size, and a
  Contact Group's or Message Tag's name, showed no description in the HTTP
  API reference. Each now says what it holds and when it is empty, and a
  check fails on a new field that has none.
- **The server refuses two requests it used to take silently.**
  An import whose source name had a space before or after it started, and
  its messages carried the space in their source. A change to a Contact
  Group or a Message Tag that named a contact or conversation by an
  impossible number left that one out without a word. Both are now refused
  as invalid, as any other source name or member the server cannot take is.
- **The HTTP API reference describes every optional field.** An
  optional field that holds a group of values or one of a set of choices,
  such as the identity a contact change links or the service it is on, had
  no description in the HTTP API reference's field list. Each now shows its
  description, as every other field does.
- **A media player can check a Preview or a Thumbnail before
  loading it.** A player that asked first what kind of file an attachment's
  Preview or Thumbnail was, and how large, was turned away when it said it
  would take only a picture or a video, while loading the file itself
  worked. It is now told the file's type and size, and that it can ask for
  part of the file.
- **Stopping the server stops the conversion it was running.** A
  server stopped with Ctrl-C or `docker stop` while it made a browser copy
  of a video left that conversion running after the server had stopped,
  using the computer for nothing. It now stops the conversion, removes the
  part-made copy, and makes the copy again when it next starts. Stopping
  the command that rebuilds the copies does the same.
- **A video's browser copy plays in every browser.** The copy
  the server made of a HEVC video, the format an iPhone records in, was HEVC
  as well, which most browsers cannot play. It is now H.264, which they all
  can. A photo or MP3 that every browser shows as it is no longer gets a
  copy it does not need.
- **Docker Compose runs as a real user when UID and GID aren't
  set.** It ran the container with an empty user and printed warnings.
- **Long conversations can be read to the end.** Messages past
  the 50,000th of a conversation could not be loaded.
- **Storage counts a shared attachment once.** One video attached
  to ten messages counted ten times in an account's storage and the Owner's
  totals.
- **A busy server no longer fails requests that only read.**
  While an import held the database, recording when a token was last used
  could fail the request itself.
- **`docker stop` lets requests finish.** The server finished
  requests in flight on Ctrl-C only, so stopping the container cut off an
  Upload or a Demo Account build.
- **The Demo Account is whole or absent.** A Demo Account build
  that was stopped part-way read as ready with part of its conversations; it
  is now removed and reported failed, so the next build starts clean. A build
  from Owner Home touches the Demo Account alone: it no longer converts other
  accounts' attachments or holds up their requests, and nobody can enter the
  Demo Account until it is complete. A reset that fails leaves the database
  as it was and usable, a Demo Data settings file with a key it does not
  use is refused, and the first group's opening line names its real title.
- **Media conversion finishes and reports failures.** A long
  conversion could hang for good, and a failure now says what ffmpeg said.
  A GIF whose type was written with capitals or extra detail is left
  animated instead of turned into a still picture. A Preview cut short by
  an interrupted run is written again, an attachment still uploading is
  left alone, and the server's `process-assets` command reports failure
  when a conversion failed. The server's `import` command fails when it
  can't read an entry in the directory, instead of leaving that conversation
  out.
- **Programs using the HTTP API get the answers its reference
  describes.** The Bearer scheme is read in any case, the health check
  answers a probe that accepts only text, a message is found by its id
  wherever it is, oversized and out-of-range requests are refused with the
  documented error, and the API reference pages carry the same headers as
  everything else.
- **`reset-demo` works on the Message Crate your configuration
  names.** It built the Demo Account into a database of its own choosing
  beside the configuration directory, whatever the configuration said, and then
  replaced the configuration file with one the server would not start with.
  It now rebuilds the Demo Account in the configured database and leaves the
  file alone. Before it puts the rebuilt database in place, it checks every
  row of every other account and the Server Settings, and refuses if any of
  them changed.
- **Rebuilding the Demo Account no longer holds up other
  accounts.** The rebuild deleted the old Demo Account in one step and then
  compacted the whole database file, and on the large Demo Data another
  account's Upload or edit could wait long enough to fail. The old Demo
  Account's messages are now deleted a few thousand at a time, so other
  accounts' changes go through in between. Only `reset-demo`, which runs
  while the server is stopped, still compacts the file; a new Message Crate
  also starts listening sooner.
- **Stopping the server during a Demo Account build no longer
  waits for all its Demo Data.**
  Stopped while a build was still making up its Demo Data, the server waited
  until all of it was written, which on the large set is the longest part of
  the build, and could leave a directory of part-written Demo Data behind. It
  now stops as soon as the conversation it is writing is done, and leaves
  nothing behind.
- **Every contact in the Demo Data's Address Book has a name.**
  One contact in the medium Demo Data's Address Book, and three in the
  large one, had a blank name. Loading the book could not name them, so
  their numbers stayed under Unknown. Every contact in it now has a name,
  and building the medium Demo Account names all 75.
- **The Docker image no longer sets environment variables the
  server never reads.** The image set `MC_DB`, `MC_DATA_DIR` and `HOSTNAME`,
  and changing them changed nothing. The database and the data directory come
  from `[paths]` in the configuration, and the address the server listens
  on from `[server]`, as they always did.
- **Programs using the HTTP API get a conversation's first and
  last message times once each.** A conversation carried its last message's
  time twice. When no message was left once duplicates are set aside, it
  sent one of those times with no value rather than leaving it out. It now
  carries its first and last message times once each, and leaves both out
  when there is no message to date them.
- **Programs using the HTTP API can read a Contact Group,
  Message Tag or Saved Search by its id.** Creating one answered with the
  address that holds its id, and reading that address was refused. It now
  answers the item as the list shows it, and answers "not found" for one
  that belongs to another account.
- **Removing or changing messages right after an import no
  longer fails with "no such table: messages".** It failed now and then
  when an import had just finished on the same server.
- **A Demo Account that fails to build on a first start is
  removed and leaves nothing behind.** On a first start, a Demo Account that
  failed to build then could not be removed, and its files stayed behind.
- **The configuration reference states each request body limit as
  the server applies it.** It said the attachment size limit was also the
  limit on every other request body. It limits only an attachment upload.
  Logging in, creating an account, claiming a Message Crate, an address book
  load and every other request each have a limit fixed in the server, which
  the page now lists.
- **A Preview cut short is made again without `--force`.** A
  Preview left part-written by a stopped `process-assets` run was kept and
  shown as it was until someone ran the command with `--force`. Every run
  now checks each Preview against its contents and makes a damaged one
  again, and removes the part-written files a stopped run or import left in
  the attachment directories.
- **How the desktop app checks that its server started was
  reworked, with nothing visible.**
- **The server's `reset-demo` command checks more of what it
  must leave alone.** Before it puts the rebuilt Demo Account in place, it
  checks that nothing else changed. That check now also covers the Audit
  Trail of deleted accounts, what a search finds in other accounts'
  messages, and the files in other accounts' directories. The reset stops if any
  of them changed. On a database of about 1.3 million messages the check
  takes about 14 seconds.
- **Deleting attachments no longer holds up everyone else.**
  Emptying the Trash, deleting a conversation or all of an account's
  messages, and the clean-up at the end of an import deleted every file
  while keeping all other changes waiting. With many files on a slow disk,
  another person's import, a sign-in or media conversion could wait 15
  seconds and fail. The files are now set aside in a moment and deleted
  afterwards, while everything else goes on.
- **Checking on an attachment upload no longer reads the whole
  file again.** When the file being uploaded was already stored, asking
  how far the upload had got, sending one of its parts or cancelling it
  read and checked every byte of the stored file first, and threw the
  answer away. For a large video that was the whole file on every check.
  Those steps now read only the upload's own record.
- **The server refuses an attachment directory setting that is a
  path.** The configuration's `assets_dir` and `assets_converted_dir` each
  name one directory inside every account's directory, but an absolute path
  was accepted there. It put every account's attachments in one directory,
  and `reset-demo` failed with "prepared reset state is incomplete". The
  server now refuses to start when either is absolute, contains a
  separator or a `:`, starts with `.` (as `.` and `..` do), ends in `.` or
  a space, or is empty, or when both are the same name in any letter case,
  and the message names the setting.
- **A damaged Preview whose original is gone is no longer
  shown.** When `process-assets` found a Preview that does not match its
  contents and the original it was made from was missing, it could not make
  the Preview again, and the attachment went on showing the damaged one.
  The run now deletes that Preview, so the attachment shows as one with no
  Preview, and still reports it among the failures.

### Upgrading

- If a script passes `--staging-dir` to the server's `import` command,
  change it to `--input`, because the command no longer takes that name.
  `--dir` and `--export-dir` still work.
- An export, or a conversion with no output directory chosen, now goes into
  the Export Directory; look for it there, through Settings → System, rather
  than in the Staging Directory.
- An Import Run's log is now in the Logs Directory, beside the Export
  Directory in the operating system's app-data directory, rather than in the
  run's directory. The Import screen's Import log link opens it.
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
- If you have a program that reads an account from the HTTP API, it must
  read each entry of `phones` as an object: the number is in `address`, and
  `services` names `phone` (Text Message), `whatsapp`, or both. A number on
  both services is one entry, where it used to be the same string twice.
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
- JSON and JSONL message files exported before orphaned messages had
  conversations of their own are refused when you import or convert them,
  rather than read with a person named "orphaned", and so is an Import Run an
  earlier build left paused. CSV, EML and mbox files carry no version, so one
  exported by an earlier build is still read, and its "orphaned" conversation
  comes in as a person of that name. Export the backup again with this build,
  then import or convert the new files; discard a paused run and start the
  import again.
- An Apple Messages backup imported by an earlier build keeps its one
  "orphaned" conversation and the contact named "orphaned". To get a
  conversation for each sender instead, use **Delete all messages** in
  Settings, then import the backup again.
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
