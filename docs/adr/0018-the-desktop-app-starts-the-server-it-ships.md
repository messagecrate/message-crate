# The desktop app starts the server it ships

A person who installs the desktop app has a working Message Crate with no
second install (#970). The installer carries `message-crate-server` and the
built website beside the app. When the app is pointed at its own address,
`127.0.0.1:8080`, it asks that address what it is: a Message Crate that
answers is used as it is; if nothing answers, the app starts the server as a
separate process and stops it when the app closes; if another program holds
the port, the app says so and starts nothing.

## Why

Hosting stays Docker's job. The app's server exists so that one install is
enough to read messages on the computer the app is on, and it runs only while
the app is open: no background service, no start on boot.

It is the same program the Docker image runs, started with `serve --data-dir
--bind --static-dir` and no config file. A Message Crate behaves the same
whichever way it was started: the same first start with the Demo Account, the
same login card, the same Owner Home. The two differ only in what starts the
server and where it listens.

The port is 8080 in both, so there is one address to document and the app's
default address needs no change. That is also why a Message Crate already
answering there is used rather than treated as a clash: a person with Docker
on the same computer gets the Message Crate they already have.

The data is in the operating system's app-data directory, under `data`, and the
app has a button that opens it. A directory under Documents is often synced by
OneDrive or iCloud, which can corrupt a database that is in use.
A dev build (`cargo tauri dev`) uses `data-dev` beside it instead, because
both builds share one app-data directory and a branch with another Schema
Fingerprint would rebuild the installed app's database empty.

The server listens on this computer only. A setting in the app opens it to
the network, for a person who wants to read from a phone while the app is
open; it is off by default and says, where it is switched on, that the
connection is plain HTTP. The app still asks `127.0.0.1:8080` what is
running, whichever way the setting is.

## Considered and rejected

**Running the server inside the app's process.** No second program to ship
or to be warned about by the operating system. Rejected because it makes a
second kind of server, built and tested differently from the one in the
Docker image, and because a server crash would take the app down with it.

**A port of its own, or a free port at each start.** Either avoids the clash
with Docker. Rejected because a second fixed port is a second number to
explain, and a changing port breaks bookmarks and the saved login.

**A background service that outlives the app.** It would let a phone reach
the Message Crate at any time. Rejected because that is hosting, which the
Docker image already does, and a service is far harder to install, update and
remove than an app.

## Consequences

The server ends one of two ways.

- **The app closes normally.** The app kills the server. Its database
  survives that, and the only work a kill interrupts is an import the closing
  app was running. The kill does not reach an ffmpeg the server runs; #1737
  is about ending that too. (The note of 2026-10-05 below replaces these two
  sentences.)
- **The app is gone without closing**, because it crashed or was killed. The
  app starts the server with `--exit-with-parent` and the app's process id,
  so the server notices and stops itself, the way it stops on Ctrl-C or
  SIGTERM: ffmpeg stopped and the work in flight finished (#1934). On Unix it
  checks its parent process every two seconds. On Windows it waits on the
  app's process handle, which the system signals the moment the app ends.

A server started any other way, by Docker or `./scripts/run-dev.sh`, is not
given the flag and watches nothing.

The installer grows by the size of the server program. The server and the app
are always the same version, since they are built together.

The server program is unsigned, like the app (#1019). ffmpeg is not shipped
(#1017); the server does without it when it is missing.

## Note, 2026-10-05: the kill takes the server's process tree

The kill now reaches the processes the server started, ffmpeg among them
(#1737). On Unix the app starts the server as the leader of a new process
group and kills the group. On Windows it puts the server in a Job Object set
to kill every process in it when the job's last handle closes, and kills the
job. The server is still killed, not asked. A server that exits on its own,
such as one that crashes, takes its processes with it too: on Unix the app
kills the group when it finds the server exited, before it reaps the server,
and on Windows the app closes the job.

The Job Object also changes the crash case on Windows: when the app is gone,
the system closes its handle to the job, which kills the server and its
ffmpeg at once, usually before the server's own wait on the app's process
handle stops it. A Windows server whose app crashed therefore ends the way a
closed app's server does, without finishing the work in flight. On Unix a
crash is unchanged.

## Note, 2026-10-07: the saved server address decides

The app starts its server only when the address on the login card is its
own, and that address is now the saved server address, a setting kept apart
from the saved login (#1972). Logging out, or a login the server refuses,
leaves it as it is. An app pointed at a Message Crate elsewhere therefore
starts nothing at every later start. The very first start, before any
address is saved, still starts and seeds the app's own Message Crate; no
start-up option skips it.
