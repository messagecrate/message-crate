import { describe, expect, it } from "vitest";
import { serverName } from "./serverName";

describe("serverName", () => {
  it("is the host and port without the scheme", () => {
    expect(serverName("http://localhost:8080")).toBe("localhost:8080");
    expect(serverName("http://192.168.1.20:9000/")).toBe("192.168.1.20:9000");
    expect(serverName("  http://127.0.0.1:8080  ")).toBe("127.0.0.1:8080");
  });

  it("names only the port the address names", () => {
    expect(serverName("http://crate.example.com")).toBe("crate.example.com");
    expect(serverName("https://crate.example.com")).toBe("crate.example.com");
    expect(serverName("https://crate.example.com:8443")).toBe("crate.example.com:8443");
  });

  it("keeps a path, so two Message Crates behind one host read apart", () => {
    expect(serverName("http://host:8080/a")).toBe("host:8080/a");
    expect(serverName("http://host:8080/crate/")).toBe("host:8080/crate");
  });

  it("keeps an IPv6 host in its brackets", () => {
    expect(serverName("http://[::1]:8080")).toBe("[::1]:8080");
  });

  it("repeats an address typed without a scheme as it was typed", () => {
    // `localhost:8080` parses as the scheme `localhost:` with no host.
    expect(serverName("localhost:8080")).toBe("localhost:8080");
    expect(serverName("my-pc:8080")).toBe("my-pc:8080");
    expect(serverName("192.168.1.20:9000")).toBe("192.168.1.20:9000");
  });

  it("repeats an address it cannot read as it was typed", () => {
    expect(serverName(" not a url ")).toBe("not a url");
  });

  it("has no name for a blank address", () => {
    expect(serverName("   ")).toBe("");
  });
});
