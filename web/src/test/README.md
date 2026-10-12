# Web unit tests are told, never wait

A unit test in `web/src` never depends on time. It is told when the thing it
waits for has happened, then reads the screen at once with `getBy…` or
`queryBy…`. It does not poll the screen, and a timeout is never widened to make
a test pass. The test files that still poll are being converted under #2902,
and a new test follows this page from the start.

*The short version: `startFakeClock()` before `render`, `await settle()` after
anything that starts work, `advanceTimersAndSettle(ms)` past a debounce or a
poll, and `deferred()` when the test decides when a fake answers. The helpers
are in `fakeTimers.ts` and `deferred.ts` in this directory.*

## Why `waitFor` and `findBy…` aren't used

`waitFor` and every `findBy…` and `findAllBy…` query poll the screen every
50 ms until the check passes or their timeout runs out, 1000 ms unless the test
passes another. So each one is a timed wait, whatever the number. A polling
test passes on an idle machine and fails on a busy one, because there the
screen reaches the state after the timeout (#1654, #2881, #2888). Widening the
timeout only moves the load at which the test fails.
`waitForElementToBeRemoved` polls the same way and isn't used either.

A test that is told when the state is reached has no timeout to lose to. On a
busy machine it runs slower and passes the same way.

## The setup

`startFakeClock()` puts the test on Vitest's fake clock. `setTimeout`,
`setInterval`, animation frames and `Date` move only when the test moves them,
and `Date` starts at the same moment in every run (`FAKE_CLOCK_START`).
It lends the clock to Testing Library, which ends each event with a
`setTimeout(0)`. It also has TanStack Query tell the screen about an answer in a
microtask instead of a `setTimeout(0)`. The reason: the fake clock runs a
zero-delay timer that was set while it was moving 1 ms later, so a poll's answer
would land only after the clock moved again. `stopFakeClock()` undoes all of it
and calls `vi.unstubAllGlobals`.

```tsx
import { advanceTimersAndSettle, settle, startFakeClock, stopFakeClock } from "../test/fakeTimers";

beforeEach(() => {
  startFakeClock();
});

afterEach(() => {
  stopFakeClock();
});
```

`setupUser()` from `user.ts` fires events with no delay between them, so
`user.click` works on the fake clock as it is.

`settle()` lets everything already started land without moving the clock: a
mock's answer, the render it causes, a transition, a refetch, and whatever those
start in turn, up to the next timer due later than now. When it returns, the
screen has drawn the result.

## Each kind of wait and how the test is told

### A mock's answer

The screen asks a mocked route function and draws the answer. A resolved mock
answers in a microtask, so `settle()` after the render is the notification.

```tsx
getDemoAccount.mockResolvedValue({ status: "failed", size: null, error: "disk full" });
renderCard();
await settle();
expect(screen.getByRole("alert")).toHaveTextContent("disk full");
```

When the test must see the screen before the answer, the mock returns a
`deferred()` promise. The test resolves it when it chooses, then settles. The
`settle()` after the resolve is required even when the test expects the answer
to change nothing, such as a late answer the screen must ignore: without it the
answer never lands, and the test passes whether the screen ignores it or not.

```tsx
const health = deferred<boolean>();
checkServerHealthMock.mockReturnValue(health.promise);
renderScreen();
await settle();
expect(screen.getByText(/^Connecting to /)).toBeInTheDocument();
health.resolve(true);
await settle();
expect(screen.getByText(/^Connected to /)).toBeInTheDocument();
```

### A timer: a debounce, a poll, or a delay the screen waits out

The source exports the delay as a constant, and the test moves the clock by it.
Moving to 1 ms short of it first shows that the screen waits for it.

```tsx
getDemoAccount.mockResolvedValueOnce(building).mockResolvedValue(ready);
renderCard();
await settle();
await advanceTimersAndSettle(BUILD_POLL_MS - 1);
expect(screen.getByRole("status")).toHaveTextContent("Building the Demo Account");
await advanceTimersAndSettle(1);
expect(screen.getByRole("button", { name: "Reset Demo Account" })).toBeEnabled();
```

A screen that decides with `Date.now()` how long to keep polling reads the
fake clock too, so moving the clock moves it.

### A transition or other deferred render

`startTransition`, `useDeferredValue`, a state update in an effect, and a
`Suspense` boundary waiting on a lazy component all finish inside `act`.
`settle()` runs inside `act`, so it finishes them too. A lazy chunk's importer
is a function the test supplies, and the promise it returns lands in the same
`settle()`.

```tsx
const Arrives = lazyChunk("SettingsScreen", async () => ({ default: () => <p>Settings</p> }));
render(
  <Suspense fallback={null}>
    <Arrives />
  </Suspense>,
);
await settle();
expect(screen.getByText("Settings")).toBeInTheDocument();
```

### A mutation's follow-up refetch

A mutation's answer marks a cache entry stale, and the screen asks again. The
refetch is a mock's answer that the first answer started, so one `settle()`
after the click lands the write, the refetch, and the render.

```tsx
let trashed = [contact(7, "Grace Hopper")];
listContactsMock.mockImplementation(async () => contactPage(trashed));
restoreContactMock.mockImplementation(async () => {
  trashed = [];
});
renderAt("/trash");
await settle();
await user.click(screen.getByRole("button", { name: "Restore Grace Hopper" }));
await settle();
expect(screen.getByText("No contacts in Trash")).toBeTruthy();
```

### A user event's effect

`await user.click(…)` returns once the event is handled, inside Testing
Library's own `act`. What the handler does at once (a dialog opening, a mock
called) is already on the screen. `settle()` follows an event that starts async
work. The test keeps it there even where the effect happens to land without it,
so the test does not lean on how user-event wraps its events.

### Nothing to wait for

A state that is on the screen when `render` or the event returns needs no
notification: a React Aria popover that opens on the click, a section that
expands, a form drawn before anything answers. The test reads it at once with
`getBy…`. Such a test can't be checked by dropping a `settle()`, because it
has none to drop. Breaking the source is what shows it guards something.

### A store outside React that the test changes

When the test changes a store the screen subscribes to (such as releasing a
held desktop job), it does so inside `act`, and the screen has drawn the change
when `act` returns.

```tsx
act(() => release());
expect(screen.getByRole("button", { name: "Upload to Message Crate" })).toBeEnabled();
```

### A fake that is reached after work the test cannot see

Where a mock is called only after a chain the test doesn't drive, the fake
resolves a `deferred()` when it is reached, and the test awaits that inside
`act`.

```tsx
const cancelAsked = deferred();
cancelMock.mockImplementation(() => cancelAsked.resolve());
await user.click(screen.getByRole("button", { name: "Log out" }));
await act(() => cancelAsked.promise);
```

### A hook rendered with `renderHook`

A hook test is told the same way as a screen test, and reads `result.current`
where a screen test reads the screen.

```tsx
vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true }));
const { result } = renderHook(() => useServerHealth(""));
await advanceTimersAndSettle(HEALTH_URL_DEBOUNCE_MS - 1);
expect(result.current).toBe("unknown");
await advanceTimersAndSettle(1);
expect(result.current).toBe("ok");
```

### A real sleep in the test

`await new Promise((resolve) => setTimeout(resolve, 0))` inside `act` waits for
real time too: it hopes everything queued lands before the timer fires. On the
fake clock it never fires at all. `settle()` replaces it, because `settle()`
runs what is queued rather than racing it.

### Something the fake clock doesn't drive

Real I/O, a dynamic `import()`, or an event from outside the page has no
notification on its own. The test gives it one before it relies on it: it
awaits the import before `render`, or the fake that stands in for the outside
event resolves a `deferred()`. Where neither works, the component exposes the
event it already has, such as a callback prop or a returned promise, for the
test to await, or the test moves to Playwright. Polling doesn't help here:
`waitFor` still races its timeout, and that race is the reason for this page.

## Checking a test

Each test must fail when the behavior it guards is broken. The check
breaks the source (skips the call, drops the poll, changes the delay) or drops
the notification (`settle()`, the clock move), runs the test, and sees it fail.
A test that still passes guards nothing.

Dropping a `settle()` proves less than breaking the source. A test whose state
is drawn at first render, or whose screen before the answer already looks like
the state it checks, passes without its `settle()` and still guards the
behavior, which only a source break shows.
