---
title: Back up an Android phone
description: Write an Android phone's SMS and MMS to an XML file with SMS Backup & Restore, and copy it to the computer.
---

An Android phone's SMS and MMS are read from the XML file written by the app **SMS Backup & Restore**, by SyncTech.
The app is free, needs no root access, and is the only route this guide walks through for Android.

This step ends with the backup's XML file on the computer.

WhatsApp is a different source with its own page: [WhatsApp](/docs/user/import-sources/whatsapp/).

:::caution[Steps inside the app are not tested]
The steps on the phone follow [SyncTech's documentation](https://www.synctech.com.au/sms-backup-restore/). The labels in the app change between versions. A wrong step is worth [an issue](https://github.com/messagecrate/message-crate/issues).
:::

## Make the backup on the phone

1. Install [SMS Backup & Restore](https://play.google.com/store/apps/details?id=com.riteshsahu.SMSBackupRestore) from Google Play and allow the permissions it asks for. It can't read messages without them.
2. Select **Set up a backup**.
3. Keep **Messages** on. **Phone calls** can be off, because Import skips call logs.
4. Under the advanced options for messages, keep media included, so photos and videos sent by MMS are in the backup.
5. Choose where the backup is written. **Your phone** writes it to the phone's storage. Google Drive, Dropbox, and OneDrive also work and save the copying in the next section.
6. Start the backup and wait for it to finish.

The app's own encryption must stay off.
The desktop app can't open an encrypted backup, because it has no field for that password.

## Copy the file to the computer

Each backup is one file named like `sms-20261001120000.xml`.

- From **Your phone**: connect the phone with a cable, choose file transfer on the phone, and copy the `.xml` file from the directory the app wrote it to.
- From cloud storage: download the `.xml` file from that service's website.

Import takes that one `.xml` file, not a directory and not a `.zip`.
A `.zip` must be unpacked first.
An older backup is a file of its own, and each one is imported on its own.

## Note the phone's own number

Import asks for the phone numbers that belonged to the phone.
The XML doesn't record which number was the owner's, and Import uses the answer to tell sent messages from received ones.
A phone that changed numbers over the years has several, and each one belongs on the list.

## Check that it worked

The computer holds the backup's `.xml` file, and the file is larger than a few kilobytes.

Next: [Import the backup](/docs/user/your-messages/import-your-backup/).
