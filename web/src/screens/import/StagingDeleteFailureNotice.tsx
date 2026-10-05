import Button from "../../components/Button";
import type { StagingDeleteFailure } from "./importRunStore";

type StagingDeleteFailureNoticeProps = {
  failure: StagingDeleteFailure;
  onDismiss: () => void;
};

/**
 * Says that a run directory of a discarded, cancelled or finished Import
 * Run is still on disk, and why. A run directory can hold several
 * gigabytes, so a failed delete is never dropped without a word.
 */
export default function StagingDeleteFailureNotice({
  failure,
  onDismiss,
}: StagingDeleteFailureNoticeProps) {
  return (
    <div
      className="mb-5 rounded-md border border-danger-soft-border bg-danger-soft-bg p-3"
      role="alert"
    >
      <p className="m-0 text-[0.813rem] text-danger">
        Message Crate could not delete this Import Run's directory{" "}
        <code className="font-mono text-[0.75rem] break-all">{failure.path}</code>
        {`: ${failure.reason}. Delete it by hand to free the space it takes.`}
      </p>
      <Button variant="secondary" size="xs" className="mt-2" onPress={onDismiss}>
        Dismiss
      </Button>
    </div>
  );
}
