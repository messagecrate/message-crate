# Web unit tests are told, never wait

A unit test in `web/src` never depends on time. It is told when the thing it
waits for has happened, then reads the screen at once with `getBy…` or
`queryBy…`. It does not poll the screen, and a timeout is never widened to make
a test pass. A Biome check enforces this ("The check").

*The short version: `startFakeClock()` before `render`, `await settle()` after
anything that starts work, `advanceTimersAndSettle(ms)` past a debounce or a
poll, `advanceToNextFrame()` past an animation frame, and `deferred()` when the
test decides when a fake answers. The helpers are in `fakeTimers.ts` and
`deferred.ts` in this directory.*

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

## The check

`web/biome-plugins/noTimedWaitsInTests.grit` is a Biome plugin that
`web/biome.json` runs on every test file in `web/src`. `biome ci` fails on each
call to `waitFor`, `vi.waitFor`, `vi.waitUntil`, `waitForElementToBeRemoved`, a
`findBy…` or `findAllBy…` query, `setTimeout` or `setInterval`. Each error names
the file, the line, and the section of this page that says what to write
instead. `check-pr.sh` and CI's web job run it.

A test can still need a real timer on purpose. `SearchBar.test.tsx` has a
parent that sets its value on a later task, because that lateness is the bug
the test guards (#1000). Such a call opts out with a comment that gives its
reason, and the fake clock still runs the timer:

```tsx
// biome-ignore lint/plugin/noTimedWaitsInTests: the parent under test is late on purpose (#1000); the fake clock runs this timer when the test moves it
setTimeout(() => setValue(q), 0);
```

A string such as `toFake: ["setTimeout"]` is not a call, so the check passes
it.

Biome reports a plugin that fails to run as an info, not an error, so a
broken pattern or a Biome upgrade could switch the check off without failing
`biome ci`. `noTimedWaitsInTests.canary.ts` beside the plugin holds every form
it catches, each with its error suppressed. A form the plugin stops catching
leaves its suppression unused, and `check-pr.sh` and CI lint that file with
`--error-on-warnings`, so the unused suppression fails them. A form added to
the plugin gets a line in the canary.

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

### An animation frame

Some work waits for the next animation frame. React Aria puts focus back on a
menu's trigger a frame after the menu closes. A drawer measures a column on the
frame after a scroll. `startFakeClock()` fakes `requestAnimationFrame`, so the
frame comes only when the test moves the clock to it. `advanceToNextFrame()`
makes that move inside `act`.

The source has no constant for a frame to move the clock by, as "A timer: a
debounce, a poll, or a delay the screen waits out" has. `settle()` doesn't move the clock, so `settle()`
never runs a frame. Reading the screen before the move shows that the screen
waits for the frame. Where a promise or a `MutationObserver` report schedules
the frame, `settle()` lands that report first.

```tsx
await user.keyboard("{Escape}");
expect(document.activeElement).not.toBe(trigger);
advanceToNextFrame();
expect(document.activeElement).toBe(trigger);
```

### Two things stamped with `Date.now()`

The fake clock stands still between moves, so two things the code stamps with
`Date.now()` get the same time unless the test moves the clock between them.
No timer is waiting here. TanStack Query stamps each write with the time it
started (`submittedAt`), and a hook that reports the newer of two writes'
errors compares those stamps. The test moves the clock by 1 ms before the
second write, which stands for the person starting it later. It reads the
first error before the second write starts, so the last check shows the newer
error replacing one already reported, not the only error there.

```tsx
await expect(result.current.create("Work")).rejects.toThrow("create failed");
await settle();
expect(result.current.error?.message).toBe("create failed");
await advanceTimersAndSettle(1);
await expect(result.current.rename("Family", "Fam")).rejects.toThrow("rename failed");
await settle();
expect(result.current.error?.message).toBe("rename failed");
```

When both stamps fall inside one chain the test does not step through, such
as an Export Run's start time and its export step, the fake that stands for
the earlier step moves the clock with `vi.setSystemTime`. That moves `Date`
and fires no timer, so a time read after that step is later than it.

```tsx
invokeExport.mockImplementationOnce(async () => {
  exportStepAt = Date.now();
  vi.setSystemTime(exportStepAt + 1);
});
```

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

A mock that answers still lands in such a test, after the test returns and
before `cleanup`, because `startFakeClock` has TanStack Query tell the screen
in a microtask. When that answer draws more than the test reads, the test
spends time drawing it, and the screen may need providers the test never gave
it. The mock for a test that reads the first draw returns a promise that
never settles, and a test about the answer gives one.

```tsx
beforeEach(() => {
  getAccountProfileMock.mockReturnValue(new Promise(() => {}));
});
```

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
where a screen test reads the screen. A hook test file whose every wait is a
promise it can await inside `act` (an action the hook returns, or a
`deferred()` a fake resolves) needs no fake clock, as
`useImportJob.test.tsx` shows. One that waits on a timer or a refetch starts
the clock as above.

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
runs what is queued rather than racing it. A sleep for a frame,
`await new Promise((resolve) => requestAnimationFrame(resolve))`, becomes a
call to `advanceToNextFrame()` ("An animation frame"). Where the calls the
sleep waited on return promises, as a hook's actions do, the test awaits
those inside `act` instead, and needs no clock at all.

```tsx
await act(async () => {
  const first = result.current.approve();
  const second = result.current.approve();
  transcode.answer({ summary: "Transcode finished.", transcode: undefined });
  await Promise.all([first, second]);
});
```

### Something the fake clock doesn't drive

Real I/O, a dynamic `import()`, or an event from outside the page has no
notification on its own. The test gives it one before it relies on it: it
awaits the import before `render`, or the fake that stands in for the outside
event resolves a `deferred()`. Where neither works, the component exposes the
event it already has, such as a callback prop or a returned promise, for the
test to await, or the test moves to Playwright. Polling doesn't help here:
`waitFor` still races its timeout, and that race is the reason for this page.

A screen that loads a lazy chunk of its own, such as Owner Home opening an
account's Settings, imports the chunk's module the first time it is shown.
That first import reads files, which the fake clock doesn't drive. The test
file imports the same module once before any test, on the real clock, so the
screen's import finds it loaded and lands within a `settle()`.

```tsx
beforeAll(async () => {
  await import("./SettingsScreen");
});
```

## Checking a test

Each test must fail when the behavior it guards is broken. The check
breaks the source (skips the call, drops the poll, changes the delay) or drops
the notification (`settle()`, the clock move), runs the test, and sees it fail.
A test that still passes guards nothing.

Dropping a `settle()` proves less than breaking the source. A test whose state
is drawn at first render, or whose screen before the answer already looks like
the state it checks, passes without its `settle()` and still guards the
behavior, which only a source break shows.
