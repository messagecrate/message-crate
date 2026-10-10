import type { AttachmentMediaMode } from "../../lib/types";

/**
 * The four attachment choices: what happens to attachments, and the three
 * limits Compress works to. The names match the import's form values, so the
 * choices spread into them as they are.
 */
export type AttachmentChoices = {
  attachmentMedia: AttachmentMediaMode;
  maxResolution: string;
  maxFps: string;
  minSizeMb: string;
};

/** The attachment choices out of a value that carries them, such as restored form values. */
export function attachmentChoicesOf(values: AttachmentChoices): AttachmentChoices {
  return {
    attachmentMedia: values.attachmentMedia,
    maxResolution: values.maxResolution,
    maxFps: values.maxFps,
    minSizeMb: values.minSizeMb,
  };
}
