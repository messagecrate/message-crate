import type { AttachmentMediaMode } from "./types";

type AttachmentStepCopy = {
  label: string;
  doneDetail: string;
};

/** Step title and done detail for the Import Attachments setting. */
export function attachmentStepCopy(mode: AttachmentMediaMode): AttachmentStepCopy {
  switch (mode) {
    case "skip":
      return {
        label: "Skip attachments",
        doneDetail: "Message attachments skipped",
      };
    case "copy":
      return {
        label: "Copy attachments",
        doneDetail: "Copied attachments",
      };
    case "convert":
    case "compress":
      return {
        label: "Convert attachments",
        doneDetail: "Attachments processed",
      };
    default: {
      const _exhaustive: never = mode;
      return _exhaustive;
    }
  }
}
