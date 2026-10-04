/**
 * Sets `name` on `el` to `value`, or removes it when there is none.
 *
 * React Aria's components pass only the attributes they know to their element,
 * so `title` and `aria-current` given to a `Button` or a `ListBoxItem` never
 * reach the page. A ref sets them on the element instead.
 */
export function keepAttribute(
  el: HTMLElement,
  name: string,
  value: string | boolean | undefined,
): void {
  if (value === undefined || value === false || value === "") {
    el.removeAttribute(name);
  } else if (el.getAttribute(name) !== String(value)) {
    el.setAttribute(name, String(value));
  }
}
