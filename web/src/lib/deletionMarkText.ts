import { sourceLabel } from "./exportSources";

/** What an Unsent message reads in place of its text, in the conversation and in the Messages list. */
export const UNSENT_TEXT = "Unsent";

/**
 * The note on a message Deleted in the source app, naming the source as the
 * product does: "Deleted in Apple Messages". The conversation puts it beside
 * the time and the Messages list under the text, so both read alike.
 */
export function deletedInSourceText(source: string): string {
  return `Deleted in ${sourceLabel(source)}`;
}
