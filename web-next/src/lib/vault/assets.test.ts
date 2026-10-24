import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { type FetchPath, proxyAsset, sha256FromPath } from "./assets";

const SHA = "b".repeat(64);

describe("proxyAsset", () => {
  it("answers the version asked for, cached", async () => {
    const asked: string[] = [];
    const fetchPath: FetchPath = async (path) => {
      asked.push(path);
      return new Response("preview", {
        headers: { "content-type": "audio/mpeg" },
      });
    };
    const res = await proxyAsset(SHA, "preview", new Headers(), fetchPath);
    assert.equal(res.status, 200);
    assert.equal(await res.text(), "preview");
    assert.equal(res.headers.get("content-type"), "audio/mpeg");
    assert.deepEqual(asked, [`/v1/assets/${SHA}/preview`]);
    assert.match(res.headers.get("cache-control") ?? "", /immutable/);
  });

  it("passes a byte range through, both ways", async () => {
    let sent: Record<string, string> = {};
    const fetchPath: FetchPath = async (_path, headers) => {
      sent = headers;
      return new Response("ab", {
        status: 206,
        headers: {
          "content-range": "bytes 0-1/10",
          "accept-ranges": "bytes",
          "content-type": "video/mp4",
        },
      });
    };
    const res = await proxyAsset(
      SHA,
      "preview",
      new Headers({ range: "bytes=0-1", cookie: "mv_session=x" }),
      fetchPath,
    );
    assert.deepEqual(sent, { range: "bytes=0-1" });
    assert.equal(res.status, 206);
    assert.equal(res.headers.get("content-range"), "bytes 0-1/10");
    assert.equal(res.headers.get("accept-ranges"), "bytes");
  });

  it("answers 404 for a Thumbnail the server has not made", async () => {
    const asked: string[] = [];
    const fetchPath: FetchPath = async (path) => {
      asked.push(path);
      return new Response('{"status":404}', { status: 404 });
    };
    const res = await proxyAsset(SHA, "thumbnail", new Headers(), fetchPath);
    assert.equal(res.status, 404);
    assert.deepEqual(asked, [`/v1/assets/${SHA}/thumbnail`]);
  });

  it("answers 401 when the server refuses the Session", async () => {
    const res = await proxyAsset(
      SHA,
      "original",
      new Headers(),
      async () => new Response(null, { status: 401 }),
    );
    assert.equal(res.status, 401);
  });
});

describe("sha256FromPath", () => {
  it("reads the sha256 the path ends in", () => {
    assert.equal(sha256FromPath(["imessage", SHA]), SHA);
    assert.equal(sha256FromPath(["imessage", "photo.jpg"]), null);
    assert.equal(sha256FromPath([]), null);
  });
});
