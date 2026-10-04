/**
 * Putting a value into a field in one event, for tests whose subject is the
 * value rather than the typing.
 *
 * `user.type` fires one input event per character, and each one renders the
 * screen again. That is real work with no timer to skip, so a long value on a
 * busy machine can outrun a test's 5000 ms budget on its own (#297, #1417).
 * A paste runs the same validation once.
 */

import type { UserEvent } from "@testing-library/user-event";

/** Click `field`, as a person does before typing, and paste `text` into it. */
export async function fill(user: UserEvent, field: HTMLElement, text: string): Promise<void> {
  await user.click(field);
  await user.paste(text);
}
