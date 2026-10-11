// The Docker HEALTHCHECK of the release image (docker/Dockerfile).
//
// It asks /health at the address the container's server listens on, read from
// /proc, so the check follows the server wherever `[server] bind` or
// `serve --bind` puts it and needs no edit when the port changes. It runs on
// the node the base image already carries, so the image needs no curl.
//
// The container's command is the oldest message-crate-server process. When
// that command is not `serve` (`docker compose run --rm server reset-demo`,
// `create-owner`), or the container runs no message-crate-server at all (a
// shell started with `--entrypoint bash`), nothing is meant to answer, so the
// check passes instead of reading unhealthy once the start period ends. A
// serving container whose server has died has already exited, because tini
// is PID 1 and the entrypoint execs the server, so a missing server never
// means one that stopped answering. A `serve` that is not listening yet,
// while it builds the Demo Account on a first start, fails the check;
// failures inside --start-period do not count.
//
// Exit 0 is healthy, 1 unhealthy; what it printed shows in
// `docker inspect --format '{{json .State.Health}}' <container>`.

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const SERVER = "message-crate-server";
// The `st` column of /proc/net/tcp and tcp6 holds the kernel's TCP state as
// hex; 0A is TCP_LISTEN.
const TCP_STATE_LISTEN = "0A";

function argsOf(pid) {
  try {
    return fs.readFileSync(`/proc/${pid}/cmdline`, "utf8").split("\0").filter((arg) => arg !== "");
  } catch {
    return [];
  }
}

function serverProcess() {
  const pids = fs
    .readdirSync("/proc")
    .filter((name) => /^\d+$/.test(name))
    .map(Number)
    .sort((a, b) => a - b);
  for (const pid of pids) {
    const args = argsOf(pid);
    if (args.length > 0 && path.basename(args[0]) === SERVER) {
      return { pid, args };
    }
  }
  return null;
}

function socketInodes(pid) {
  const inodes = new Set();
  for (const fd of fs.readdirSync(`/proc/${pid}/fd`)) {
    try {
      const match = /^socket:\[(\d+)\]$/.exec(fs.readlinkSync(`/proc/${pid}/fd/${fd}`));
      if (match) inodes.add(match[1]);
    } catch {
      // The descriptor closed between the listing and the read.
    }
  }
  return inodes;
}

// /proc/net/tcp writes an address as hex 32-bit words in the host's byte
// order. This returns the address's bytes in network order.
function addressBytes(hex) {
  const out = [];
  for (let i = 0; i < hex.length; i += 8) {
    const word = Buffer.from(hex.slice(i, i + 8), "hex");
    out.push(os.endianness() === "LE" ? word.reverse() : word);
  }
  return Buffer.concat(out);
}

// The hosts to ask, in order. A dual-stack socket on the IPv6 wildcard
// answers on 127.0.0.1 too, and ::1 does not exist when the container has
// IPv6 turned off, so 127.0.0.1 is asked first.
function hostsOf(hex) {
  const bytes = addressBytes(hex);
  if (bytes.length === 4) {
    const v4 = [...bytes].join(".");
    return [v4 === "0.0.0.0" ? "127.0.0.1" : v4];
  }
  if (bytes.subarray(0, 10).every((b) => b === 0) && bytes[10] === 0xff && bytes[11] === 0xff) {
    return [[...bytes.subarray(12)].join(".")];
  }
  if (bytes.every((b) => b === 0)) return ["127.0.0.1", "[::1]"];
  const groups = [];
  for (let i = 0; i < 16; i += 2) groups.push(bytes.readUInt16BE(i).toString(16));
  return [`[${groups.join(":")}]`];
}

function listenAddresses(pid, inodes) {
  for (const table of ["tcp", "tcp6"]) {
    let rows;
    try {
      rows = fs.readFileSync(`/proc/${pid}/net/${table}`, "utf8").trim().split("\n").slice(1);
    } catch {
      continue;
    }
    for (const row of rows) {
      const cols = row.trim().split(/\s+/);
      const [addressHex, portHex] = cols[1].split(":");
      if (cols[3] === TCP_STATE_LISTEN && inodes.has(cols[9])) {
        const port = Number.parseInt(portHex, 16);
        return hostsOf(addressHex).map((host) => `${host}:${port}`);
      }
    }
  }
  return [];
}

function exit(code, message) {
  console.log(message);
  process.exit(code);
}

async function main() {
  const server = serverProcess();
  if (server === null) exit(0, `no ${SERVER} process runs, so nothing serves to check`);
  const command = server.args[1] ?? "";
  if (command !== "serve") {
    exit(0, `the container runs \`${SERVER} ${command}\`, which serves nothing to check`);
  }
  const addresses = listenAddresses(server.pid, socketInodes(server.pid));
  if (addresses.length === 0) exit(1, `${SERVER} serve is not listening yet`);
  const failures = [];
  for (const address of addresses) {
    const url = `http://${address}/health`;
    try {
      const response = await fetch(url, { signal: AbortSignal.timeout(2000) });
      if (response.ok) exit(0, `${url} answered ${response.status}`);
      failures.push(`${url} answered ${response.status}`);
    } catch (error) {
      failures.push(`${url}: ${error.cause?.message ?? error.message}`);
    }
  }
  exit(1, failures.join("; "));
}

main();
