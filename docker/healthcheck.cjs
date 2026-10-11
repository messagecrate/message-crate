// The Docker HEALTHCHECK of the release image (docker/Dockerfile).
//
// It asks /health at the address the container's server listens on, read from
// /proc, so the check follows the server wherever `[server] bind` or
// `serve --bind` puts it, and the port is written down in one place only:
// the config. It runs on the node the base image already carries, so the
// image needs no curl.
//
// The container's command is the oldest message-crate-server process. When
// that command is not `serve` (`docker compose run --rm server reset-demo`,
// `create-owner`), nothing is meant to answer, so the check passes instead of
// reading unhealthy once the start period ends. A `serve` that is not
// listening yet, while it builds the Demo Account on a first start, fails the
// check; failures inside --start-period do not count.
//
// Exit 0 is healthy, 1 unhealthy; what it printed shows in
// `docker inspect --format '{{json .State.Health}}' <container>`.

const fs = require("node:fs");
const path = require("node:path");

const SERVER = "message-crate-server";
const LISTEN = "0A";

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

// /proc/net/tcp writes an address as hex 32-bit words in host byte order,
// which is little-endian on every platform the image is built for.
function words(hex) {
  const out = [];
  for (let i = 0; i < hex.length; i += 8) {
    out.push(Buffer.from(hex.slice(i, i + 8), "hex").reverse());
  }
  return Buffer.concat(out);
}

function hostOf(hex) {
  const bytes = words(hex);
  if (bytes.length === 4) {
    const v4 = [...bytes].join(".");
    return v4 === "0.0.0.0" ? "127.0.0.1" : v4;
  }
  if (bytes.subarray(0, 10).every((b) => b === 0) && bytes[10] === 0xff && bytes[11] === 0xff) {
    return [...bytes.subarray(12)].join(".");
  }
  if (bytes.every((b) => b === 0)) return "[::1]";
  const groups = [];
  for (let i = 0; i < 16; i += 2) groups.push(bytes.readUInt16BE(i).toString(16));
  return `[${groups.join(":")}]`;
}

function listenAddress(pid, inodes) {
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
      if (cols[3] === LISTEN && inodes.has(cols[9])) {
        return `${hostOf(addressHex)}:${Number.parseInt(portHex, 16)}`;
      }
    }
  }
  return null;
}

function fail(message) {
  console.log(message);
  process.exit(1);
}

const server = serverProcess();
if (server === null) fail(`no ${SERVER} process is running`);
const command = server.args[1] ?? "";
if (command !== "serve") {
  console.log(`the container runs \`${SERVER} ${command}\`, which serves nothing to check`);
  process.exit(0);
}
const address = listenAddress(server.pid, socketInodes(server.pid));
if (address === null) fail(`${SERVER} serve is not listening yet`);
const url = `http://${address}/health`;
fetch(url, { signal: AbortSignal.timeout(4000) }).then(
  (response) => {
    console.log(`${url} answered ${response.status}`);
    process.exit(response.ok ? 0 : 1);
  },
  (error) => fail(`${url}: ${error.cause?.message ?? error.message}`),
);
