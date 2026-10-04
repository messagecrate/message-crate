# Media

How an attachment is kept, served and shown: which versions of it exist, when
the server makes them, which one a screen loads, and how a video or a voice
note reaches its player. The words Asset, Preview, Thumbnail and Media Link
are defined in `CONTEXT.md`. The maintainer decided the five rules below on
#1029, on 2026-10-04; each carries its reason.

The server side of rule 1 is built (#1658). Rules 3 and 4, making the
Thumbnails and Previews after each import, are #1659. The web app's half of
rules 1, 2 and 5 is #1660, which follows both.

Every rule holds for the server in Docker and for the server the desktop app
starts alike. They are one binary, and no rule here depends on which one
answers.

## 1. A video or a voice note streams

The two routes that answer an attachment's bytes, `GET /v1/assets/{sha256}`
and `GET /v1/assets/{sha256}/preview`, answer HTTP range requests. A video
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
answers URLs that read the asset and its Preview with no header. What a Media
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

Why: the viewer used to fetch the original and fall back to the Preview only
when the browser failed to show it. The result depended on the browser, so a
HEIC photo opened as its Preview in Chrome and as the original in Safari, and
every fallback cost a failed load first. A rule on the type is known before
any byte is fetched and gives every browser the same answer.

Why the download is the original: the original is the record of what was
sent, and a Preview is a converted copy that may have lost quality or
metadata.

## 3. An attachment has up to three versions

- The **Thumbnail**, about 560 pixels across and tens of kilobytes: the image
  scaled down, or a video's first frame. Every image and video has one. The
  conversation shows it.
- The **Preview**, a copy a browser can play or show. Only an attachment of a
  type in rule 2 that browsers often cannot show has one.
- The **original**, as it was imported, never changed.

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
