import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { proxyAsset, sha256FromPath } from "./assets";

const SHA = "b".repeat(64);

function answer(status: number, body: string, type = "image/jpeg"): Response {
  return new Response(status === 200 ? body : null, {
    status,
    headers: status === 200 ? { "content-type": type } : {},
  });
}

/** A fake server holding the given bodies by `/v1` path. */
function server(bodies: Record<string, string>) {
  const asked: string[] = [];
  const fetchPath = async (path: string) => {
    asked.push(path);
    const body = bodies[path];
    return body === undefined ? answer(404, "") : answer(200, body);
  };
  return { asked, fetchPath };
}

describe("proxyAsset", () => {
  it("answers the Preview the server holds, cached", async () => {
    const { asked, fetchPath } = server({
      [`/v1/assets/${SHA}/preview`]: "preview",
    });
    const res = await proxyAsset(SHA, "preview", fetchPath);
    assert.equal(res.status, 200);
    assert.equal(await res.text(), "preview");
    assert.deepEqual(asked, [`/v1/assets/${SHA}/preview`]);
    assert.match(res.headers.get("cache-control") ?? "", /immutable/);
  });

  it("answers the original, uncached, when the server holds no Thumbnail", async () => {
    const { asked, fetchPath } = server({ [`/v1/assets/${SHA}`]: "original" });
    const res = await proxyAsset(SHA, "thumbnail", fetchPath);
    assert.equal(res.status, 200);
    assert.equal(await res.text(), "original");
    assert.deepEqual(asked, [
      `/v1/assets/${SHA}/thumbnail`,
      `/v1/assets/${SHA}`,
    ]);
    assert.equal(res.headers.get("cache-control"), "private, no-store");
  });

  it("answers 404 when the original is missing too", async () => {
    const { fetchPath } = server({});
    const res = await proxyAsset(SHA, "preview", fetchPath);
    assert.equal(res.status, 404);
  });

  it("answers 401 when the server refuses the Session", async () => {
    const res = await proxyAsset(SHA, "original", async () => answer(401, ""));
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
