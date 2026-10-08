# The desktop app downloads the programs it needs and ships none of them

The desktop app needs two programs it does not contain: ffmpeg (with
ffprobe) and wtsexporter. The app uses ffmpeg in an Import Run's Media
Stage, which converts or compresses attachments. The server it starts uses
ffmpeg to make Previews and Thumbnails of Assets.
wtsexporter reads WhatsApp data. Each time the app starts it checks for both
and downloads what is missing into the Tools Directory, in the background,
with no button and no prompt (#1053). It passes the Tools Directory to the
server it starts. The server looks on `PATH` and then there, as the app does,
so both find the same ffmpeg. Each download is pinned in the app to one
release and one checksum, and a file that doesn't match is refused.
wtsexporter is always the app's own copy. ffmpeg is not downloaded when it is
on `PATH`.

Built in #2001. The app records beside the programs which release and
checksum each file it wrote came from.

## Why

The Media Stage asks ffmpeg for `libx264` and `libx265`. The server asks for
`libx264` to make a video's Preview. Only a GPL build of ffmpeg has those
encoders. The repository is under the Fair Core License, so Message Crate
must not hand out that build, in an installer or from a host of its own.
When the person's computer fetches it from a third party, Message Crate
distributes nothing.

A Message Crate the app starts must behave like one Docker runs, and the
Docker image has ffmpeg. An app that waits for the person to install ffmpeg
leaves the two routes different for most people.

A click to get functionality was refused. Most people never open Settings,
and a WhatsApp import that first asks for a download is one more step in the
hardest import there is. Because nobody chooses the download, the pinned
checksum is what stands between the app and a file that was changed on the
way.

ffmpeg on `PATH` is used as it is, because a person who installed ffmpeg has
chosen it, and 80 to 100 MB is a large download to repeat for nothing.
wtsexporter gets no such rule: a `pipx` install whose Python has gone is
still found on `PATH` and fails only when it is run.

## Considered and rejected

**Shipping both in the installer,** as the Apple Messages Reader is. It works
with no internet, and it was rejected because it makes Message Crate a
distributor of a GPL ffmpeg build and adds about 100 MB to every installer.

**Shipping wtsexporter and downloading ffmpeg.** wtsexporter is small and
under the MIT licence, so nothing forbids it. It was rejected because two
programs would then reach the computer by two mechanisms.

**A download button in Settings and on the Import form.** It was rejected
because it is a click to get functionality.

**An LGPL build of ffmpeg.** It could be shipped, and it was rejected because
it has neither `libx264` nor `libx265`. Without them the Media Stage could
not re-encode a video. The server could not make a video's Preview either.

**Keeping the ffmpeg directory setting as an override.** It was rejected
because `PATH` is already the first place the app looks, so the setting
would be a second way to say the same thing.

## Consequences

On a new computer, the first WhatsApp import and the first Media Stage need
an internet connection, and both depend on two GitHub projects keeping their
release files in place: `eugeneware/ffmpeg-static` and
`KnugiHK/WhatsApp-Chat-Exporter`. When a download fails the app says so only
where the program is needed, and the user guide's troubleshooting section
tells a person how to install ffmpeg with a package manager or put either
program in the Tools Directory by hand. Convert needs no connection. It
rewrites already-exported files and leaves their attachments as they are, so
it runs neither program.

Until ffmpeg arrives on a new computer, the server the app starts makes no
Previews or Thumbnails. The Assets wait in its queue and are made once
ffmpeg is there, at the server's next start or after the next Import Run.

The Tools Directory belongs to the app. A release that pins a newer version
replaces what is there, whoever put it there, and deletes the old file only
after the new one has passed its checksum.

Moving to a newer ffmpeg or wtsexporter is a change to the app: a new pinned
release and checksum for every platform, in one pull request.

ffmpeg from `PATH` is whatever version the person installed, so the app can
run a version no release was tested with.

## Amended 2026-10-08: wtsexporter comes from Message Crate's fork

The text above is kept as it was decided. Since #2001, the wtsexporter
the app downloads is a pinned release of Message Crate's fork,
`messagecrate/WhatsApp-Chat-Exporter`, and not of
`KnugiHK/WhatsApp-Chat-Exporter`. The fork adds to its JSON who sent each
message and who is in each group, because the original's JSON does not say
who sent a group message and the import needs to know. The fork's changes stay
in the fork, and the fork is archived once the original has everything
Message Crate needs (#1053). Nothing else in this decision changed.
