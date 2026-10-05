import { sourceLabel } from "./exportSources";

/** What an Unsent message reads in place of its text, in the conversation and in the Messages list. */
export const UNSENT_TEXT = "Unsent";

/**
 * The note on a message Deleted in the source app: "Deleted in" and the
 * source's name from `sourceLabel`, such as "Deleted in Apple Messages". The
 * conversation shows it after the message's time, and the Messages list on a
 * line under the message's text.
 */
export function deletedInSourceText(source: string): string {
  return `Deleted in ${sourceLabel(source)}`;
}
