import type { AttachmentChoices } from "./types";

/** The choices a new Import form starts with. */
export const DEFAULT_ATTACHMENT_CHOICES: AttachmentChoices = {
  attachmentMedia: "copy",
  maxResolution: "720p",
  maxFps: "30",
  minSizeMb: "20",
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
