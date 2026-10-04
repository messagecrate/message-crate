# Media

How an attachment is kept, served and shown: which versions of it exist, when
the server makes them, which one a screen loads, and how a video or a voice
note reaches its player. The words Asset, Preview, Thumbnail and Media Link
are defined in `CONTEXT.md`. The maintainer decided the five rules below on
#1029, on 2026-10-04; each carries its reason.

The server side of rule 1 is built (#1658), and so are rules 3 and 4, the
Thumbnails and Previews the server makes after each import (#1659), and the
web app's half of rules 1, 2 and 5 (#1660).

Every rule holds for the server in Docker and for the server the desktop app
starts alike. They are one binary, and no rule here depends on which one
answers.

## 1. A video or a voice note streams

The three routes that answer an attachment's bytes, `GET /v1/assets/{sha256}`,
`GET /v1/assets/{sha256}/preview` and `GET /v1/assets/{sha256}/thumbnail`,
answer HTTP range requests. A video
and a voice note play from a `<video>` or `<audio>` element that loads its
own `src`, never from a whole file fetched first and handed over as a blob.

Why: a blob is the whole file. A large video downloaded completely before its
first frame showed, and seeking worked only once it had. A media element
asks for the file a range at a time, starts playing on the first range, and
asks for another range to seek.

How the server answers a range, and why, is a rule of the HTTP interface:
`docs/architecture/http-api.md`, "Status codes".

A media element cannot send the Session's `Authorization` header, so the web
app puts a Media Link in `src`: `POST /v1/assets/{sha256}/media-links`
answers URLs that read the asset, its Preview and its Thumbnail with no
header. What a Media
Link is, why it was chosen over a cookie or the Session token in the URL, and
what it reaches is in `docs/architecture/http-api.md`, "Credentials and
reach".

## 2. The viewer chooses by file type, never by a failed load

Opening an attachment shows the original when it is of a type every browser
shows: JPEG, PNG, GIF, WebP, MP4 with H.264 video, MP3. It shows the Preview
when it is of a type browsers often cannot show: HEIC, HEVC video in a
`.mov`, AMR audio and the like. When such an attachment has no Preview yet,
the viewer says so and offers the download. Downloading always gives the
original.

The server makes Previews by the same rule, in `media::browser_shows`: the
types every browser shows are exactly JPEG, PNG, GIF and WebP images, MP3
audio, and MP4 with H.264 video, and every other image, video or audio file
gets a Preview. The type comes from the stored file's extension, then the
type the import declared, then the name the export gave the file, the order
the server reads a type everywhere. Only an MP4 is opened, by ffprobe, to read
its video codec, so a HEVC video in an `.mp4` gets a Preview too. The list is
exact rather than generous: AAC audio in an `.m4a` plays in most browsers and
still gets a Preview, because a list that holds a type most browsers show
would fail in the one that does not, and an MP3 copy of a voice note costs
little.

The web app reads the rule in `fullVersion` (`web/src/lib/attachmentMedia.ts`).
It opens the Preview when the attachment has one (`preview_mime_type`), the original when its type is one of the six above, and neither otherwise.
It reads the type in the server's order for a stored original, which is named by its fingerprint alone and has no extension: the type the import declared, then the extension of the export's name for the file, then of its path in the export.
It cannot read an MP4's codec, so a HEVC MP4 plays its original until its Preview is made, and its Preview after.

Why: the viewer used to fetch the original and fall back to the Preview only
when the browser failed to show it. The result depended on the browser, so a
HEIC photo opened as its Preview in Chrome and as the original in Safari, and
every fallback cost a failed load first. A rule on the type is known before
any byte is fetched and gives every browser the same answer.

Why the download is the original: the original is the record of what was
sent, and a Preview is a converted copy that may have lost quality or
metadata.

## 3. An attachment has up to three versions

- The **Thumbnail**, a JPEG at most 560 pixels on its long side and tens of
  kilobytes: the image scaled down, never enlarged, or a video's first frame.
  Every image and video has one, a GIF included. The conversation shows it.
- The **Preview**, a copy a browser can play or show: a JPEG of an image, an
  MP4 of a video with H.264 video at most 1080p and AAC audio, an MP3 of
  audio. Only an attachment of a type in rule 2 that browsers often cannot
  show has one. A GIF has none, because a still copy of an animation is not
  the animation.
- The **original**, as it was imported, never changed.

The Thumbnail and the Preview are stored in the account's converted
directory, each named by the SHA-256 of its own bytes, and the attachment
rows of the original name them. A client addresses both by the original's
fingerprint, `GET /v1/assets/{sha256}/thumbnail` and
`GET /v1/assets/{sha256}/preview`, and an attachment says each exists in
`thumbnail_mime_type` and `preview_mime_type`. A route answers
`404 Not Found` until its version is made.

Why a JPEG Thumbnail and not a WebP: every ffmpeg build writes JPEG, and a
Thumbnail of 560 pixels is tens of kilobytes either way. Why the Preview of a
video is H.264 and not HEVC: browsers that play HEVC are the exception, and a
Preview exists for the browser that cannot play the original.

Why a Thumbnail: a conversation shows many attachments at once, and a phone
photo or video is megabytes. A Thumbnail is enough for the size the
conversation shows it at, and small enough that a long conversation scrolls
without loading megabytes it never shows whole.

Why a Preview only where it is needed: an original every browser shows is
already the best copy there is. Converting it as well would cost disk space
and give a copy no better than the original.

## 4. The server makes them after each import

After each import, the server makes a Thumbnail for every image and video the
import brought, and a Preview for each that rule 2 says needs one. It works
in the background, so the Import Run finishes without waiting for it. The
server in Docker and the server the desktop app starts both do it.
`process-assets` stays, for rebuilding the Thumbnails and Previews and for
repairing missing ones.

How: an Import Run that ends, completed or discarded, adds the Assets its
messages name to the `media_queue` table, one row per account and
fingerprint, and wakes the pass. The `import` command adds its run's Assets
the same way, and the next `serve` works on them. The pass runs on a thread
of its own, so a long conversion never holds a request. It takes the Assets
oldest first, makes what each still needs, shares a Thumbnail or Preview the
original already has with rows that do not name it yet, and removes the row.
An Asset whose conversion fails leaves the queue too, with the failure in the
server's log, and `process-assets` tries it again. An Asset a later Import
Run queues while the pass works on it is queued again rather than dropped,
so the new run's rows get the versions too. The pass holds no database
connection while ffmpeg runs, and writes its part-made files in a work
directory under the data directory. A server that stops, on Ctrl-C or
SIGTERM, kills the ffmpeg the pass runs and waits for it, removes the work
directory, and leaves the Asset queued for its next start. A Demo Account
build the server is running when it stops has its ffmpeg killed the same
way, and the part-built Demo Account is removed. `process-assets` stopped
the same way kills its ffmpeg, removes its work directory and fails; a
second Ctrl-C or SIGTERM ends it at once and leaves its work directory for
the next pass. A process that is killed does none of this. The next pass
removes the work directory it left behind. Nothing is stored for an account
deleted meanwhile. A version made for an attachment deleted meanwhile is
named by no row, and the sweep at the next Import Run's end removes it once
it is an hour old, the grace that keeps another pass's file with the same
bytes.

Why stop the conversion: ffmpeg is a process of its own, and one the server
does not stop goes on converting after the server has stopped, using the
computer for work nothing will record.

Why a table: the queue outlives the process, so a server stopped part-way
works through what was left when it starts again, with nothing to redo and no
Import Run to replay.

Without ffmpeg the pass makes nothing and leaves the queue as it is, so the
Assets wait for a server that has it, at its next start or after the next
Import Run. The Docker image has ffmpeg. The desktop app's server looks for
it where the `media` crate does: beside the program, in `MESSAGE_CRATE_BIN`,
then on `PATH` (`docs/adr/0019-the-desktop-app-downloads-the-programs-it-needs.md`
is the decided way it gets there).

Why after each import: a Preview existed only once someone ran
`process-assets` by hand, so an attachment imported with **Attachments →
Copy** had none until then.

Why the server and not the desktop app: every import ends at the server,
whichever program sent it, a program holding an API token included, and only
the server holds every account's assets.

Why in the background: converting a video takes far longer than importing
its message, and an Import Run held open for it would keep the person
waiting for work they can do without.

## 5. Each screen loads only what it shows

- A conversation fetches a Thumbnail only when its message scrolls near the
  screen, and never fetches an original while scrolling.
- Opening an image loads the original or the Preview by rule 2, and keeps the
  Thumbnail on screen until it is ready. The viewer also fetches the next and
  the previous attachment.
- A video in a conversation shows its Thumbnail with a play button, and
  streams only when it is played.
- Audio gets a player in the conversation.

Why: a long conversation holds thousands of attachments, and loading any of
them before it is near the screen spends the bandwidth and memory of the
whole conversation on the part in view. Keeping the Thumbnail up while the
full image loads means the viewer never shows an empty frame. Fetching the
neighbours means stepping through a conversation's photos does not wait on
each one. A video streams only on play, because most videos in a
conversation are scrolled past and never played. Audio has no picture to
show as a Thumbnail, and a voice note shown as a file chip could not be
played in place.
