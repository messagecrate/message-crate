import { describe, expect, it } from "vitest";
import { isPlainHttpOverNetwork } from "./plainHttp";

describe("isPlainHttpOverNetwork", () => {
  it("is false for a loopback address, which never leaves this computer", () => {
    for (const address of [
      "http://localhost:8080",
      "http://127.0.0.1:8080",
      "http://127.4.5.6",
      "http://[::1]:8080",
      "http://[::ffff:127.0.0.1]:8080",
      "http://app.localhost:8080",
      "http://0.0.0.0:8080",
    ]) {
      expect(isPlainHttpOverNetwork(address), address).toBe(false);
    }
  });

  it("is true for plain HTTP to another computer, by address or by name", () => {
    for (const address of [
      "http://192.168.1.20:8080",
      "http://messages.home.arpa:8080",
      "http://[fe80::1]:8080",
    ]) {
      expect(isPlainHttpOverNetwork(address), address).toBe(true);
    }
  });

  it("is false for any https address, which is encrypted", () => {
    for (const address of ["https://192.168.1.20:8080", "https://messages.example.com"]) {
      expect(isPlainHttpOverNetwork(address), address).toBe(false);
    }
  });

  it("is false for text that is not an address", () => {
    for (const address of ["", "192.168.1.20:8080", "not an address"]) {
      expect(isPlainHttpOverNetwork(address), address).toBe(false);
    }
  });
});
