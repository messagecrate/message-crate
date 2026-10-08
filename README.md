[![Issues][issues-shield]][issues-url]
[![project_license][license-shield]][license-url]
[![source_available][source-available-shield]][license-url]

<a id="readme-top"></a>

<br />
<div align="center">
  <a href="https://github.com/messagecrate/message-crate">
    <img src="docs/img/icon.png" alt="Message Crate icon" width="250" height="250">
  </a>

<h1 align="center">Message Crate</h1>
  <p align="center">
    Your messages, out of the apps.
    <br />
    <br />
    <a href="https://messagecrate.app/docs/user/"><strong>Explore the docs »</strong></a>
    <br />
    <a href="https://messagecrate.app/docs/user/try/what-is-message-crate/">Try Message Crate</a>
    &middot;
    <a href="https://github.com/messagecrate/message-crate/issues/new?labels=bug&template=bug_report.md">Report Bug</a>
    &middot;
    <a href="https://github.com/messagecrate/message-crate/issues/new?labels=enhancement&template=feature_request.md">Request Feature</a>
  </p>
</div>

<!-- TABLE OF CONTENTS -->
<details>
  <summary>Table of Contents</summary>
  <ol>
    <li><a href="#about-the-project">About The Project</a></li>
    <li><a href="#who-the-project-is-for">Who The Project Is For</a></li>
    <li><a href="#getting-started">Getting Started</a></li>
    <li><a href="#faq">FAQ</a></li>
    <li><a href="#contributing">Contributing</a></li>
    <li><a href="#additional-documentation">Additional documentation</a></li>
    <li><a href="#license">License</a></li>
    <li><a href="#how-message-crate-is-built">How Message Crate Is Built</a></li>
    <li><a href="#project-status">Project Status</a></li>
    <li><a href="#maintainers">Maintainers</a></li>
    <li><a href="#related-projects">Related Projects</a></li>
  </ol>
</details>

## About The Project

[![Docker][Docker]][Docker-url] [![React][React.js]][React-url] [![Rust][Rust-dev]][Rust-url] [![SQLite][SQLite]][SQLite-url] [![Tauri][Tauri]][Tauri-url] [![Vite][Vite]][Vite-url]

Message Crate copies conversations out of chat apps and phone backups (iMessage, WhatsApp, Android SMS) and stores them in a searchable archive on a computer its owner controls.
Old threads open in a browser, years of messages are searchable, and everything exports back out as ordinary files.

Message Crate runs on the owner's own hardware.
There is no cloud service, no account with Bitrealm, and no AI feature in the product.
Messages never leave the computer they are imported on, unless the owner runs the server on another computer of their own.

### Project Details

The Message Crate software has three parts:

- **Backend** - The server. It stores the messages, keeps accounts logged in, and runs the search.
- **Desktop App** - The program that imports messages into a Message Crate from phone backups and app exports. It also reads, organizes, and exports them.
- **Website** - The same screens in a web browser, for reading, searching, and organizing messages. Importing, exporting, and converting need the desktop app, because they read and write files on the computer.

Message Crate imports:

- Apple Messages from an iPhone backup, or from Messages on a Mac
- Android texts and picture messages from an SMS Backup & Restore file
- WhatsApp from an iPhone backup or from WhatsApp's Android files

A few older export formats still import, for anyone whose phone is long gone and only an old export remains.

Once messages are imported:

- Threads read the way they did in the app or on the phone, including group chats. Photos, videos, and other attachments are included.
- Search covers years of conversations
- Export saves a copy back out as ordinary files in a directory on disk
- Texts from more than one phone or app combine into one archive

## Who The Project Is For

This project is for people who want a personal copy of their phone messages. That includes anyone replacing a phone, leaving a chat app, or keeping a long-term archive of texts.

## Getting Started

The [User Guide](https://messagecrate.app/docs/user/try/what-is-message-crate/) starts with the demo data and ends with an import of the reader's own backup.

Which way to run it depends on who uses it:

| Situation | What to run |
|---|---|
| One person, one computer | The desktop app. It starts a Message Crate on that computer, and a browser on the same computer can open it too. |
| Several people, or a computer that is always on | The server in Docker, following [Run Message Crate with Docker](https://messagecrate.app/docs/user/features/owner/run-with-docker/). Any browser on the home network opens it, and the desktop app connects to it for imports. |

It is the same archive, the same search, and the same exports either way.

The [Developer Guide](https://messagecrate.app/docs/developer/) covers setting up a local development environment and compiling from source.

## FAQ

**Is there a cloud version?**
No. Message Crate runs on a computer its owner controls, and there is no hosted service and no account with Bitrealm.

**Does Message Crate use AI?**
No. The product has no AI feature, and no message is sent to an AI service.
AI is used to build the product, as [How Message Crate Is Built](#how-message-crate-is-built) describes.

**Where do the messages go?**
Into a SQLite database and an attachment directory on the computer that runs the server.
The user guide's [What Message Crate is](https://messagecrate.app/docs/user/try/what-is-message-crate/) page describes the pieces.

**Is Docker required?**
No. The desktop app carries the server and starts it.
Docker is for a Message Crate shared by several people or kept on a computer that is always on.

**Can the messages be taken back out?**
Yes. [Export](https://messagecrate.app/docs/user/features/messages/export/) writes them as ordinary files: JSON, JSONL, CSV, EML, or MBOX.
Nothing is locked in.

**Is it open source?**
No. It is source-available under the Fair Core License, as [License](#license) explains: the code can be read, changed, built, and run for the reader's own use, but not offered as a competing product.
Each version becomes Apache 2.0 two years after its release.

**Will the free, self-hosted version go away?**
No. The self-hosted Message Crate is the product, not a trial, and the maintainer keeps their own messages in one.

**Is it secure? Has it been audited?**
No third party has audited the code.
Security is a design priority, but Message Crate is built for a home network, not for the open internet.
Its login only keeps the accounts on one Message Crate apart, and the Demo Account logs in with an empty password.
[Run on another machine](https://messagecrate.app/docs/user/features/owner/run-on-another-machine/) says a Message Crate reached from the internet needs a reverse proxy with TLS in front of the server.
That proxy should carry its own authentication too, because the server's login was never meant to be the only lock.

**Is it finished?**
No. Message Crate is under heavy development on the way to 1.0.
It works today, and screens, formats, and settings still change between releases.

## Contributing

Contributions are welcome. The [Contributing guide](https://messagecrate.app/docs/developer/contributing/) covers the development environment, running the code, and how pull requests work.

## Additional documentation

Most documentation lives in the guidebook at [messagecrate.app](https://messagecrate.app):

- [User Guide](https://messagecrate.app/docs/user/)
- [Developer Guide](https://messagecrate.app/docs/developer/) — including [Architecture](https://messagecrate.app/docs/developer/design/) (System Design, Message Transfer, Common message)

## License

Message Crate is **source-available, not open source**. It is distributed under the [Fair Core License 1.0](LICENSE.md) (`FCL-1.0-ALv2`): the code can be read, changed, built, and run for the reader's own use, but not offered as a product that competes with Message Crate. Two years after each version is released, that version becomes available under the Apache License 2.0.

See [LICENSE.md](LICENSE.md) for the full terms.

## How Message Crate Is Built

AI agents write most of the code, and that is not hidden.
Message Crate was built agent-first from the start, rather than written by hand and then reviewed by an AI.

The maintainer defines the architecture, records the decisions in the repository's design documents and ADRs, and reviews the larger changes.
Not every pull request gets a line-by-line human review.
CI is the gate: every change passes formatting, lints, the test suites, and the license checks before it reaches `main`.

None of this touches the product's data.
The messages in a Message Crate are never sent to an AI service, because the product has no AI feature.

## Project Status

This project is currently under heavy development and moving towards a v1.0.0 release.

## Maintainers

Matt Beisser - [hello@bitrealm.io](mailto:hello@bitrealm.io)

## Related Projects

- [ChatLab](https://github.com/ChatLab/ChatLab) - Local-first tool that analyzes chat history with AI.
- [Discord Export](https://discordexport.com/discord-user-list) - Hosted helpers for pulling Discord data, including a server member list.
- [DiscordChatExporter](https://github.com/tyrrrz/discordchatexporter) - Exports Discord channel history to HTML, JSON, CSV, and other files.
- [iMazing](https://imazing.com/) - Manages iOS devices and can export message threads from an iPhone or iTunes backup.
- [imessage-exporter](https://github.com/ReagentX/imessage-exporter) - Command-line tool that exports Apple Messages from `chat.db` to several text formats.
- [msgvault](https://github.com/kenn-io/msgvault) - Offline archive for email and chat with search and analytics on SQLite and DuckDB.
- [OMA WAP Forum](https://www.openmobilealliance.org/specifications/affiliates/wap-forum) - Specifications for WAP and MMS that some Android SMS apps still encode.
- [OpenExtract](https://www.openextract.app/) - Pulls iMessage, SMS, photos, and voicemail out of a local iPhone backup.
- [SMS Backup & Restore](https://www.synctech.com.au/sms-backup-restore) - Android app from SyncTech that writes SMS and MMS to an XML file.
- [SMS Backup+](https://github.com/jberkel/sms-backup-plus) - Android app that backs SMS and MMS up to an IMAP mailbox (often Gmail).

<p align="right">(<a href="#readme-top">back to top</a>)</p>

[issues-shield]: https://img.shields.io/github/issues/messagecrate/message-crate.svg
[issues-url]: https://github.com/messagecrate/message-crate/issues
[license-shield]: https://img.shields.io/badge/license-FCL_1.0-blue
[source-available-shield]: https://img.shields.io/badge/source--available-not_open_source-orange
[license-url]: https://github.com/messagecrate/message-crate/blob/main/LICENSE.md

[React.js]: https://img.shields.io/badge/React-%2320232a.svg?logo=react&logoColor=%2361DAFB
[React-url]: https://reactjs.org/

[Rust-dev]: https://img.shields.io/badge/Rust-%23000000.svg?e&logo=rust&logoColor=white
[Rust-url]: https://rust-lang.org/

[Tauri]: https://img.shields.io/badge/Tauri-24C8D8?logo=tauri&logoColor=fff
[Tauri-url]: https://github.com/tauri-apps/tauri

[Vite]: https://img.shields.io/badge/Vite-646CFF?logo=vite&logoColor=fff
[Vite-url]: https://vite.dev/

[SQLite]: https://img.shields.io/badge/SQLite-%2307405e.svg?logo=sqlite&logoColor=white
[SQLite-url]: https://sqlite.org/

[Docker]: https://img.shields.io/badge/Docker-2496ED?logo=docker&logoColor=fff
[Docker-url]: https://www.docker.com/
