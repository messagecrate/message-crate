---
title: WhatsApp
description: Import WhatsApp from an Android phone or an iPhone backup. Advanced, because it runs a separate program the desktop app downloads and, on Android, needs a decryption key.
---

WhatsApp is the hardest source to import.
It belongs after a first import of [iPhone](/docs/user/your-messages/back-up-an-iphone/) or [Android](/docs/user/your-messages/back-up-an-android-phone/) messages has worked, not before.

Two things make it hard:

- The program that reads WhatsApp data, `wtsexporter`, isn't part of the desktop app. The app downloads it when it starts, so the first WhatsApp import needs the app to have started once with an internet connection.
- On Android, WhatsApp's backup file is encrypted, and Import needs the key.

## wtsexporter

The desktop app runs `wtsexporter`, from Message Crate's fork of the open-source [WhatsApp Chat Exporter](https://github.com/KnugiHK/WhatsApp-Chat-Exporter), [messagecrate/WhatsApp-Chat-Exporter](https://github.com/messagecrate/WhatsApp-Chat-Exporter), because only the fork records who sent each group message, the members of each group, and the ids a quoted reply is linked by.
Import refuses a `result.json` that another `wtsexporter` wrote, and names the fork, because without those fields a group message's sender is a name or a number, never both, and the number can be an internal WhatsApp id rather than a phone number.

Each time it starts, the desktop app downloads the pinned release into its Tools Directory when it isn't there, in the background, and [**Settings → System**](/docs/user/features/settings/system/#media) shows the program or how its download stands.
The app runs `wtsexporter` only from the Tools Directory, never from `PATH`.
A WhatsApp import can't start without it, and the Import form names the reason and offers **Try again**.
An import started while `wtsexporter` is still downloading waits for the download, and its progress line says so.
A program in the Tools Directory whose checksum is the pinned program's is kept, whoever put it there, and anything else under its name is replaced by the pinned program.

## Who is imported

Each person keeps the phone number and the name the backup holds for them.

- A group's participants are its members, including a member who never wrote, and anyone else who wrote in it. The phone's owner is not one of them. A group whose member list the backup doesn't hold has the people who wrote in it.
- A group message's sender has the phone number WhatsApp stores for them. A person WhatsApp knows only by an internal id, with no phone number in the backup, is imported under that id, with their name.
- A name is the one in the phone's address book first, then the name the person set in their own WhatsApp profile.
- A one-to-one chat's person has the chat's name.
- Some old group messages name no sender in the backup, and are imported with none.

## WhatsApp on Android

### What Import needs

A directory that holds one of:

- `msgstore.db`, the WhatsApp message database, or
- `msgstore.db.crypt15` (or the older `.crypt14` or `.crypt12`), the encrypted backup of it, together with its key.

The phone number the WhatsApp account is registered to is also required, because an Android backup doesn't record it.

### Room on disk

Staging has `wtsexporter` decrypt `msgstore.db.crypt15` into the Scratch Directory, `scratch` in the operating system's app-data directory, while it reads the backup.
The decrypted database is larger than the `.crypt15` file, because the backup is compressed, and for a long-used WhatsApp account it can run to several GB.
Nothing knows that size before the database is written, so Staging can't check for room first, as it does for every other backup.
A disk that is full, or fills part-way, stops the run with `Not enough space on the disk that holds the Scratch Directory: there was no room left to finish reading this backup. Free some space on that disk and run the import again.`
The part already written is deleted with the rest of the run's working files, so the import can run again once that disk has room.

### Without root: the 64-digit key

WhatsApp can protect its backup with a key the person holds.

1. In WhatsApp, open **Settings → Chats → Chat backup → End-to-end encrypted backup** and turn it on.
2. Choose the **64-digit encryption key** in place of a password, and write the key down. A password can't be used for Import.
3. Run a backup from **Chat backup**.
4. Copy `msgstore.db.crypt15` to the computer. On the phone it is under `Android/media/com.whatsapp/WhatsApp/Databases/`. A WhatsApp `Media` directory beside `Databases` holds the photos and videos, and it is copied too when they are wanted.

The labels inside WhatsApp change between versions.
[WhatsApp's own help](https://faq.whatsapp.com/) describes the current ones.

### With root: the key file

A rooted phone gives access to WhatsApp's private storage, where the unencrypted `msgstore.db` and the `key` file live under `/data/data/com.whatsapp/`.
Rooting differs for every phone model and is outside this guide.
The [WhatsApp Chat Exporter documentation](https://github.com/KnugiHK/WhatsApp-Chat-Exporter) describes which files to copy.

### Fill in the Import form for Android

1. Open **Import** and choose **WhatsApp** as the source.
2. **Platform**: **Android**.
3. **Backup directory**: the directory that holds the `msgstore` file.
4. **Decryption key**: the 64-digit key, or the path to the key file. It is required when the directory has a `.crypt` file and no `msgstore.db`. The app never saves it.
5. **WhatsApp phone number**: the number the account is registered to. It is filled in from the profile.
6. **Contacts database**, **Media directory**, and **Message database** stay empty when those files are inside the backup directory.

## WhatsApp on iPhone

WhatsApp's data is inside the iPhone backup made in [Back up an iPhone](/docs/user/your-messages/back-up-an-iphone/).

The backup can be encrypted or not.
A backup that was encrypted for importing iPhone messages is used here as it is, with the same password.

### Fill in the Import form for iPhone

1. Open **Import** and choose **WhatsApp** as the source.
2. **Platform**: **iPhone**.
3. **Backup directory**: the device directory, the one that contains `Manifest.plist`.
4. **Encryption password**: the backup's password. It is required when the backup is encrypted, and stays empty when it is not. The app never saves it.
5. **WhatsApp Business** is turned on only for a WhatsApp Business backup.

For an encrypted backup, Import first decrypts WhatsApp's files into the import's working directory, which takes longer when the backup holds a lot of WhatsApp media. The decrypted files are deleted when the import finishes reading them.

The phone number is read from the backup.
When the backup doesn't hold it, Import uses **WhatsApp phone number** under **Processing Options (Advanced)**.

## Limits

- A run can't be cancelled while `wtsexporter` is working, or while an encrypted iPhone backup is being decrypted. It ends when that step finishes.
- Using files from two different backups together fails, because a key decrypts only the backup it was made with.
- Status updates aren't imported, because a Status is a post to many people that expires after 24 hours, not a conversation. The run summary counts them, as in `Skipped 4 status updates`. A reply to someone's Status is a message in the chat with that person and is imported.
- A quoted reply is linked to the message it quotes only when that message is in the same chat of the same backup, because the reply names it by the id the backup gave it there. A reply whose quoted message isn't there is imported as a reply that names no message.
- Channel posts aren't imported, because a Channel is a one-way feed from a publisher that nobody can write back to. The run summary counts them, as in `Skipped 3 channel posts`.

The run itself is the same as in [Import your backup](/docs/user/your-messages/import-your-backup/#start-the-import).
