/**
 * Refuse a password whose confirmation differs, with the message every form
 * that asks for a password twice shows.
 *
 * Only the mismatch is checked here. Length is the server's rule, so it stays
 * there rather than being restated and left to drift.
 */
export function requireMatchingPasswords(password: string, confirmPassword: string): void {
  if (password !== confirmPassword) {
    throw new Error("Passwords do not match.");
  }
}
