/**
 * Load the whole app again from the server, as the browser's reload does.
 * Its own module so a test can stand in for it: jsdom's `location.reload`
 * cannot be replaced.
 */
export function reloadPage(): void {
  location.reload();
}
