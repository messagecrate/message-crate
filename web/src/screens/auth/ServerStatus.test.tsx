/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import ServerStatus from "./ServerStatus";

describe("ServerStatus", () => {
  afterEach(cleanup);

  it("names the server it is talking about in every state", () => {
    const address = "http://localhost:8080";
    const { rerender } = render(<ServerStatus state="connecting" address={address} />);
    expect(screen.getByRole("status")).toHaveTextContent(/^Connecting to localhost:8080$/);

    rerender(<ServerStatus state="connected" address={address} />);
    expect(screen.getByRole("status")).toHaveTextContent(/^Connected to localhost:8080$/);

    rerender(<ServerStatus state="disconnected" address={address} />);
    expect(screen.getByRole("status")).toHaveTextContent(/^Disconnected from localhost:8080$/);

    rerender(<ServerStatus state="untested" address={address} />);
    expect(screen.getByRole("status")).toHaveTextContent(/^Not tested: localhost:8080$/);
  });

  it("stands alone when there is no address to name", () => {
    render(<ServerStatus state="untested" address="" />);
    expect(screen.getByRole("status")).toHaveTextContent(/^Not tested$/);
  });

  it("shows a starting label as it is given", () => {
    render(
      <ServerStatus
        state="connecting"
        address="http://127.0.0.1:8080"
        label="Starting Message Crate…"
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent(/^Starting Message Crate…$/);
  });

  it("keeps an untested address out of the answered colours", () => {
    render(<ServerStatus state="untested" address="http://localhost:8080" />);
    const status = screen.getByRole("status");
    expect(status).toHaveClass("text-muted");
    expect(status).not.toHaveClass("text-ok");
    expect(status).not.toHaveClass("text-danger");
    expect(status).not.toHaveClass("motion-safe:animate-pulse");
  });

  it("colours the status to agree with what it says", () => {
    const { rerender } = render(<ServerStatus state="connected" address="http://localhost:8080" />);
    expect(screen.getByRole("status")).toHaveClass("text-ok");

    rerender(<ServerStatus state="disconnected" address="http://localhost:8080" />);
    expect(screen.getByRole("status")).toHaveClass("text-danger");
  });

  it("flashes only while connecting", () => {
    const { rerender } = render(
      <ServerStatus state="connecting" address="http://localhost:8080" />,
    );
    expect(screen.getByRole("status")).toHaveClass("motion-safe:animate-pulse");

    rerender(<ServerStatus state="connected" address="http://localhost:8080" />);
    expect(screen.getByRole("status")).not.toHaveClass("motion-safe:animate-pulse");
  });

  it("announces changes to a screen reader", () => {
    render(<ServerStatus state="connecting" address="http://localhost:8080" />);
    expect(screen.getByRole("status")).toBeInTheDocument();
  });

  it("carries a caller's own placement classes", () => {
    render(
      <ServerStatus state="connected" address="http://localhost:8080" className="pl-[13px]" />,
    );
    expect(screen.getByRole("status")).toHaveClass("pl-[13px]");
  });
});
