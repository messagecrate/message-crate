/** @vitest-environment jsdom */

import { describe, expect, it } from "vitest";
import { serverName } from "./serverName";

describe("serverName", () => {
  it("is the host and port without the scheme", () => {
    expect(serverName("http://localhost:8080")).toBe("localhost:8080");
    expect(serverName("http://192.168.1.20:9000/")).toBe("192.168.1.20:9000");
    expect(serverName("  http://127.0.0.1:8080  ")).toBe("127.0.0.1:8080");
  });

  it("names the port even when the scheme implies it", () => {
    // Dropping the scheme drops what the default port was, so it is written out.
    expect(serverName("http://crate.example.com")).toBe("crate.example.com:80");
    expect(serverName("https://crate.example.com")).toBe("crate.example.com:443");
  });

  it("keeps an IPv6 host in its brackets", () => {
    expect(serverName("http://[::1]:8080")).toBe("[::1]:8080");
  });

  it("names the website's own origin for a blank address", () => {
    expect(serverName("")).toBe(serverName(window.location.origin));
  });

  it("repeats an address it cannot read as it was typed", () => {
    expect(serverName(" not a url ")).toBe("not a url");
  });
});
