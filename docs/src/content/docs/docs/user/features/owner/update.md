---
title: Update Message Crate
description: Replace the server with a newer release while keeping the data volume, update the desktop app, and what happens when a release changes the database layout.
---

A Message Crate the desktop app starts is updated by installing the new desktop app: the server is inside it.
A Message Crate in Docker has two parts to update, the server and the desktop app.
Both carry the same version number, and both should be updated to the same release.

## Before updating

Two checks come first, because an update can empty the database.

1. The [changelog](https://github.com/messagecrate/message-crate/blob/main/CHANGELOG.md) has an **Upgrading** heading under a release when that release needs something done. A release that changes the database layout rebuilds the database empty, as described [below](#when-the-database-layout-changes).
2. The phone backups the messages were imported from must still be at hand, because they are what a rebuilt Message Crate is filled from again.

For the app's own Message Crate, a copy of the data directory (**Settings → System → Open data directory**), made while the app is closed, allows going back. For Docker, a copy of the `message-crate-data` volume, made as in [Run on another machine](/docs/user/features/owner/run-on-another-machine/#save-the-volume-to-a-file), does the same.

## Update a server in Docker

The steps match the server started in [Run Message Crate with Docker](/docs/user/features/owner/run-with-docker/): a container named `message-crate` on the volume `message-crate-data`.

`docker stop` and `docker rm` remove the running server.
They leave the volume alone, so the messages stay.
`-t 40` gives the server 40 seconds to stop, whatever the old container was started with, because a stopping server waits up to 30 seconds for the work in progress, such as an import.

```bash title="Remove the old server"
docker stop -t 40 message-crate
docker rm message-crate
```

`docker pull` downloads the newest release.

```bash title="Download the new release"
docker pull bitrealm/message-crate:latest
```

The same command as the first time starts the new server on the same volume.

```bash title="Start the new server"
docker run -d --name message-crate \
  --stop-timeout 40 \
  --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  -v message-crate-data:/app/data \
  bitrealm/message-crate:latest
```

A server that was started with a different `-p` value, as in [Run on another machine](/docs/user/features/owner/run-on-another-machine/), is started again with that value.

The server adds demo data only to a volume with no database, so starting on this volume changes nothing it holds.

### Check that it worked

1. Log in as the Owner at [http://localhost:8080](http://localhost:8080).
2. Open **Server Settings**. **Version** shows the new release.

### A fixed release in place of the newest

`latest` is the newest release.
A tag with a version number, such as `bitrealm/message-crate:0.9.0`, stays on that release.
The tag has no `v` in front.

## Update the desktop app

The installers are on the [latest release on GitHub](https://github.com/messagecrate/message-crate/releases/latest), under **Assets**.
[Install the desktop app](/docs/user/try/install-the-desktop-app/) lists which file belongs to which computer.

A desktop app from a different release than the server still connects, and the server serves it.
The Owner sees the difference on the account's **Profile** tab in [Owner Home](/docs/user/features/owner/owner-home/#profile), under **App**.

## When the database layout changes

Message Crate doesn't convert an old database to a new layout.
A server whose database layout differs from the one in the volume rebuilds the database empty when it starts.

How is a rebuild recognised?

- The browser shows **Create Owner**, as it did the first time.
- **Schema fingerprint** under **Server Settings** shows a different number than before the update. The number is the same for every server with the same database layout.
- `docker logs message-crate` shows the line `The database's schema differs from this server's, so the database is rebuilt empty and its messages must be imported again`, followed by the path of the copy described below.

Everything the database held is gone from it after a rebuild:

- the Owner and every account,
- every message, conversation, and contact,
- contact names that were typed in, Contact Groups, Message Tags, Saved Searches, and the Trash,
- every API Token.

Is anything kept?

Yes. Before the rebuild, the server writes a copy of the database beside it, named after the database with `.before-rebuild-` and the old Schema Fingerprint, such as `messagecrate.db.before-rebuild-1176793189`.
A second rebuild from the same fingerprint adds `-2`, `-3` and so on, so no copy is written over.
The copy is written under its name with `.partial` added and renamed only once it is whole, so a file ending in `.partial` is a copy that was cut off, such as by the server being stopped, and holds nothing worth keeping.
If the copy can't be written, as on a full disk, the server leaves the database as it is and stops with an error naming the copy.
A new, empty database gets no copy, because it holds nothing.

The copy opens only in a release with the old database layout, such as the one that wrote it, because any other server would rebuild it too.
That release serves it again once the copy is renamed to the database's own name, with the server stopped.
The copies stay until they are deleted, and each one is as large as the database was.
A message deleted for good after the rebuild is still in a copy made before it, because deleting erases only the database the server runs on.
The copy holds it until the copy is deleted.

The way forward on the new release is the first-time path again: [create the Owner and an account](/docs/user/your-messages/create-the-owner-and-an-account/#create-the-owner), then [import the backups](/docs/user/your-messages/import-your-backup/).
