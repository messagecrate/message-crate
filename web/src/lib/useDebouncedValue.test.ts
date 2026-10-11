/** @vitest-environment jsdom */

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDebouncedValue } from "./useDebouncedValue";

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
});

afterEach(() => {
  vi.useRealTimers();
});

type Props = { value: string; ms: number };

function render(props: Props) {
  return renderHook(({ value, ms }: Props) => useDebouncedValue(value, ms), {
    initialProps: props,
  });
}

function advance(ms: number) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

describe("useDebouncedValue", () => {
  it("starts at the value it is given", () => {
    const { result } = render({ value: "a", ms: 300 });
    expect(result.current).toBe("a");
  });

  it("starts at `initial` until the wait ends", () => {
    const { result } = renderHook(() => useDebouncedValue("a", 300, ""));
    advance(299);
    expect(result.current).toBe("");
    advance(1);
    expect(result.current).toBe("a");
  });

  it("returns a value once it stopped changing for the wait", () => {
    const { result, rerender } = render({ value: "a", ms: 300 });
    rerender({ value: "ab", ms: 300 });
    advance(299);
    rerender({ value: "abc", ms: 300 });
    advance(299);
    expect(result.current).toBe("a");
    advance(1);
    expect(result.current).toBe("abc");
  });

  it("returns a value with no wait at once, and the next wait starts from it", () => {
    const { result, rerender } = render({ value: "a", ms: 300 });
    rerender({ value: "", ms: 0 });
    expect(result.current).toBe("");
    rerender({ value: "b", ms: 300 });
    expect(result.current).toBe("");
    advance(300);
    expect(result.current).toBe("b");
  });
});
