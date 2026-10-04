/**
 * The user-event user and the way values go into fields, for tests that must
 * pass on a busy machine (#297, #1417).
 *
 * user-event's default delay is a real setTimeout(0) between events, which
 * nothing in these tests needs and which a busy machine stretches.
 * `user.type` fires one input event per character, and each one renders the
 * screen again. That is real work with no timer to skip, so a long value can
 * outrun a test's 5000 ms budget on its own. A paste runs the same validation
 * once.
 */

import userEvent, { type UserEvent } from "@testing-library/user-event";

/** A user that fires each event without waiting on a timer between them. */
export function setupUser(): UserEvent {
  return userEvent.setup({ delay: null });
}

/** Click `field`, as a person does before typing, and paste `text` into it. */
export async function fill(user: UserEvent, field: HTMLElement, text: string): Promise<void> {
  await user.click(field);
  await user.paste(text);
}
