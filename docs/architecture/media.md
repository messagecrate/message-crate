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

## 2. The viewer chooses by the server's answer, never by a failed load

Opening an attachment shows the original when the server says every browser
shows it as it is: a JPEG, PNG, GIF or WebP image, an MP3, or an MP4 with
H.264 video. It shows the Preview of anything else: HEIC, HEVC video in a
`.mov` or an `.mp4`, AMR audio and the like. When such an attachment has no
Preview yet, the viewer says so and offers the download. Downloading always
gives the original.

The server makes Previews by the same rule, in `media::browser_shows`: the
types every browser shows are exactly JPEG, PNG, GIF and WebP images, MP3
audio, and MP4 with H.264 video, and every other image, video or audio file
gets a Preview. A declared type is only a claim, so the type comes first
from the file's first bytes, where they match a known signature
(`media::media_type_from_bytes`): a video declared
`application/octet-stream` is a video, gets a Thumbnail, and gets a Preview
when `media::browser_shows` says it needs one (#2805). Where one signature fits several
types, as an MP4 and an `.m4a` share one, the declared type is kept when it
is one of them, read in its usual spelling (`audio/aac` is `audio/mp4`). A
declared audio type the signature does not name still keeps the file audio,
as the signature's first audio type, because a voice note in a 3GP or MP4
container is audio and a video player would show it as an empty frame. Bytes that match no signature fall back to the stored
file's extension, then the type the import declared, then the name the
export gave the file. The server stores and serves the Asset under the same
type: each store decides it from the bytes and writes it over a first
claim the bytes rule out. A first claim the bytes allow stays, so a later
import cannot flip an `.m4a` to an MP4, and bytes that show no type keep
the first type declared for them. Every attachment row of the Asset names
the type it is served under (#2804). A declared type that is not a `type/subtype` pair of tokens
of at most 255 bytes counts as no type at all, so the file's name decides as
it would for a missing one, and with nothing known the Asset is served as
`application/octet-stream` (#2101). So a JPEG, PNG, GIF,
WebP or MP3 is shown as it is only when its first bytes are that type's
signature. One that starts as anything else gets a Preview, a file called a
GIF included (#2257). One that cannot be read keeps the decision an earlier
pass recorded, because nothing is known about its bytes, and the pass plans
it from that decision (#2804). With no decision recorded, its type alone
decides whether the pass wants a Preview of it, so a missing JPEG is not
recorded as a Preview not made, which its type never needs. An MP4 is opened by ffprobe, to read
its video codec, so a HEVC video in an `.mp4` gets a Preview too. The list is
exact rather than generous: AAC audio in an `.m4a` plays in most browsers and
still gets a Preview, because a list that holds a type most browsers show
would fail in the one that does not, and an MP3 copy of a voice note costs
little.

The `/v1` Attachment's `shown_as_is` is the one place a client reads the
decision.
The server's media pass (rule 4) decides it with `media::browser_shows`, the same call that spares the Asset a Preview.
It stores the answer on the Asset's attachment rows (`attachments.shown_as_is`), so a read never runs ffprobe.
Before each conversion, the pass decides every Asset queued since it last looked, so a photo queued behind a long video opens at once.
It is false until the pass has looked at the file.
Without ffmpeg the pass still decides every type but MP4, so a photo opens as it is on a server that cannot convert.
Only ffprobe can read an MP4's codec, so without it an MP4 keeps what an earlier pass decided, and one never looked at counts as not shown as it is.
A Demo Account build without ffmpeg decides every original of the account the same way.

The web app reads it in `fullVersion` (`web/src/lib/attachmentMedia.ts`) and keeps no list of types of its own.
It opens the original when `shown_as_is` is true, the Preview when it is false and the attachment has one (`preview_mime_type`), and neither otherwise.
A HEVC MP4 with no Preview yet therefore says that no copy a browser can show exists yet.
Why the server answers it rather than the web app repeating the list: a second list could drift from the server's, and only the server can read an MP4's video codec (#1738).

Why: the viewer used to fetch the original and fall back to the Preview only
when the browser failed to show it. The result depended on the browser, so a
HEIC photo opened as its Preview in Chrome and as the original in Safari, and
every fallback cost a failed load first. The server's answer is known before
any byte is fetched and gives every browser the same answer.

Why the download is the original: the original is the record of what was
sent, and a Preview is a converted copy that may have lost quality or
metadata.

The desktop app downloads the original itself. The window makes a Media Link
and hands its `url` to the app's `save_download` command, which shows the Save
dialog and copies the server's answer to the chosen file as it arrives. Why:
fetched into the window, a video of hundreds of megabytes was held whole
there, again as the bytes sent to the app, and again in the request the app
received (#1739). A browser has its own downloads, so the web app in a browser
still fetches the original and hands it over.

## 3. An attachment has up to three versions

- The **Thumbnail**, a JPEG at most 560 pixels on its long side and tens of
  kilobytes: the image scaled down, never enlarged, or a video's first frame.
  Every image and video has one, a GIF included. The conversation shows it.
- The **Preview**, a copy a browser can play or show: a JPEG of an image, an
  MP4 of a video with H.264 video at most 1080p and AAC audio, an MP3 of
  audio. Only an attachment of a type in rule 2 that browsers often cannot
  show has one, or one whose bytes are not its type's (rule 2). A real GIF
  has none, because a still copy of an animation is not the animation.
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

How: an Import Run that ends, completed or discarded, adds the Assets of
the attachment rows it wrote to the `media_queue` table, one row per account
and fingerprint, and wakes the pass. The rows it wrote are those it added and
the stored rows it gave their missing file, which each name the run in
`attachments.import_id`, whichever run created their message: a backup
imported again in Append mode with a photo that was missing the first time
queues that photo (#1946). The `import` command adds its run's Assets
the same way, and the next `serve` works on them. The pass runs on a thread
of its own, so a long conversion never holds a request. It takes the Assets
oldest first, makes what each still needs, shares a Thumbnail or Preview the
original already has with rows that do not name it yet, and removes the row.
An Asset whose conversion fails leaves the queue too, with the failure in the
server's log, and `process-assets` tries it again. The pass also records
why each version was not made on the Asset's attachment rows
(`attachments.preview_not_made_reason` and
`attachments.thumbnail_not_made_reason`), and the `/v1` Attachment answers it
in `preview_not_made_reason` and `thumbnail_not_made_reason`, so the viewer
says why there is no Preview. Beside each reason it records its kind
(`attachments.preview_not_made_kind` and `thumbnail_not_made_kind`):
`conversion_failed` when ffmpeg exited with an error, `stalled` when it made
no progress for `[server] transcode_stall_secs`, `ran_past_length` when it ran past twice the
file's length, `took_too_long` when a step that reads one frame passed its
limit, `original_missing`, or `server_problem` for anything else. A kind is
set and cleared with its reason, which a `CHECK` on the table holds. The
owner's read of one Import Run counts the run's attachments by kind and lists
them, each by file name, type and size with its reasons
(`docs/architecture/http-api.md`, "Credentials and reach"), so the owner sees
which files of an import were left without a version and why, and can run
`process-assets` to try again; nothing tries again on its own (#2636).
The reason is what ffmpeg said about the file, with the paths it named made
relative to the account's directories, or a phrase of the server's when
ffmpeg did not read the file, such as when the original is missing. It
never holds a path outside the data directory or the full error, whose
context names the original's path and, for a missing ffmpeg, every
directory the server looked in. It is never recorded beside a
version that works: a failed remake under `--force` of an intact version
records nothing, and neither does a pass that fails after another pass made
the version. Rows that name a version whose file is missing or damaged get
it. A version made later clears it, and so does deciding that the original
is shown as it is. A dry run records nothing, and neither does a conversion
a stop killed. An Asset a later Import Run queues while the pass works on it
is queued again rather than dropped, so the new run's rows get the versions
too. The pass holds no database connection while ffmpeg runs, and writes its
part-made files in a work directory under the data directory. A server that stops, on Ctrl-C or
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

No single Asset and no single error stops the pass (#2059, #2072, #2091).
Every ffmpeg and ffprobe run is stopped when it hangs, in the pass and in the desktop app's Media Stage alike (#2898).
A short step gets a fixed 15 seconds: ffprobe reading a file's details, an ffmpeg run that reads one frame (a Thumbnail, or the Preview of an image), and the `-version` run that finds ffmpeg and ffprobe.
A conversion that reads the whole file has no time limit: the Preview of a video or of audio, and every video or audio conversion of the Media Stage.
It is stopped when it makes no progress instead.
ffmpeg reports its position in the file (`out_time`) on its `-progress` stream, and a position that has not moved for the no-progress wait stops the run.
The wait is 2 minutes.
A server started with a config file takes it from `[server] transcode_stall_secs` (`docs/src/content/docs/docs/developer/reference/config-and-accounts.md`); a server started with `--data-dir`, and the desktop app's own conversions, use the 2 minutes.
A conversion whose position passes twice the length ffprobe reads for the file is stopped too, as looping on a damaged file.
A length ffprobe only estimated from the bitrate of the first frames doesn't count, because such an estimate can be under half the real length.
Once the position reaches the file's length, the wait becomes at least twice the time the run took to get there.
Why: with `-movflags +faststart`, ffmpeg rewrites the whole output after its last frame and reports no progress meanwhile, which takes minutes for a large video on a slow disk.
A program stopped is killed and waited for.
The conversion then fails like any other, and the reason recorded on the Asset's rows says why it was stopped: no progress for the wait, before or after converting part of the file, past twice the file's length, or a short step over its 15 seconds.
A `-version` run stopped at its 15 seconds counts as a program that does not run, so the lookup passes over it.

The Media Stage compresses a video with HEVC first, and tries H.264 after a HEVC run that failed in a way H.264 may not:

| How the HEVC run ended | Try H.264 next? |
|---|---|
| ffmpeg exited with an error, as when the encoder is missing or refused the file | Yes |
| No progress, and the position never moved from the start | Yes: most likely the HEVC encoder hung, as a hardware encoder can |
| No progress after the position moved | No: most likely the file is damaged at that point, and H.264 stalls there too |
| Past twice the file's length | No: a damaged file loops in H.264 too |
| Stopped by the person or the server | No: nothing is wrong, and the run ends as stopped, not as a failure |

Every ffmpeg and ffprobe run reads nothing from standard input and opens only local files and pipes (`-nostdin`, `-protocol_whitelist file,pipe`).
An Asset whose work panics fails the same way: it leaves the queue, the panic is logged at `error`, and the pass goes on to the next Asset.
A pass that fails or panics outside one Asset's work, such as on a database busy past its timeout, is logged and runs again after a wait, or at the next wake if that comes sooner.
The wait is 5 seconds, doubles with each failure in a row up to 5 minutes, and starts at 5 seconds again after a pass that ends cleanly.

Why stop a run that hangs: the pass works on one Asset at a time for every account, so an ffmpeg that never ended held it until the server stopped.
The Asset stayed queued, so at the next start the pass took it again and hung again.
Why watch progress rather than give a conversion time by its length: converting a two-hour video on a slow computer, such as a small NAS converting iPhone HEVC video, can take many times the video's length, and any limit by length cut such a conversion off.
A hung 5-second clip, meanwhile, waited out the shortest limit, 10 minutes.
Why the position and not the size of what ffmpeg writes: a hung ffmpeg can keep writing without moving forward.
Why twice the length: a damaged or crafted file can make ffmpeg go around the same part forever while the position still moves, and no working conversion goes past the end of its file.
Why 15 seconds for a short step: each takes well under a second on a working file, and a fixed limit is all a step that does not read the whole file needs.
Why only local files: the files come from phone backups, and a playlist or a container inside one can name a network address that ffmpeg would otherwise open.
Why run again on a timer rather than wait for an Import Run: only the end of an Import Run woke the pass, so on a server imported into once a busy database left the queue untouched until a restart.

Why stop the conversion: ffmpeg is a process of its own, and one the server
does not stop goes on converting after the server has stopped, using the
computer for work nothing will record.

Why record the reason: with it only in the server's log, the account saw an
attachment with no Preview and no explanation, and the owner found the reason
only by searching the log for the fingerprint (#2170).

Why a phrase of the server's when ffmpeg did not read the file: a missing
original is the commonest such failure, and an answer of `null` would read
as a Preview still to come, which is what the reason exists to end. The
phrase is fixed text, so it holds no path and nothing of the message.

Why a table: the queue outlives the process, so a server stopped part-way
works through what was left when it starts again, with nothing to redo and no
Import Run to replay.

Without ffmpeg the pass makes nothing and leaves the queue as it is, so the
Assets wait for a server that has it, at its next start or after the next
Import Run. It still records whether each queued original is shown as it is
(rule 2). The Docker image has ffmpeg. The desktop app's server looks for
it where the `media` crate does: on `PATH`, then in the Tools Directory the
app passes with `serve --tools-dir`, and nowhere else
(`docs/adr/0019-the-desktop-app-downloads-the-programs-it-needs.md` is the
decided way it gets there). A server started by hand has no Tools Directory
and finds ffmpeg on `PATH` or makes nothing.
ffmpeg and ffprobe come from one place, both from `PATH` or both from the
Tools Directory, because two builds of different versions would work on one
file. One found only on `PATH` and the other only in the Tools Directory is
an error naming both.

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
