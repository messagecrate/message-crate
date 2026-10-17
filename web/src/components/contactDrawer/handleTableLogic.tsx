import type { ReactNode } from "react";
import { countOf } from "../../lib/plural";
import { formatHandleDate } from "./contactDrawerTypes";

export function handleDateCell(iso: string | null | undefined, zone: string): string {
  return formatHandleDate(iso, zone) ?? "—";
}

export type RemoveIdentityTarget = {
  address: string;
  /** The service the server recorded, or null when it recorded none. */
  service: string | null;
  serviceLabel: string;
  conversationCount: number;
};

export function removeIdentityConfirmBody(target: RemoveIdentityTarget): ReactNode {
  const { address, serviceLabel, conversationCount } = target;
  const emphasisClass = "font-medium text-accent";
  const serviceId = (
    <>
      <span className={emphasisClass}>{serviceLabel}</span>{" "}
      <span className={`${emphasisClass} break-all`}>{address}</span>
    </>
  );
  if (conversationCount <= 0) {
    return (
      <p className="mt-3 text-[0.875rem] leading-relaxed text-muted">
        Removing {serviceId} will unlink it from this contact.
      </p>
    );
  }
  return (
    <p className="mt-3 text-[0.875rem] leading-relaxed text-muted">
      Removing {serviceId} will unlink {countOf(conversationCount, "conversation")} from this
      contact. Unlinked data will not be deleted.
    </p>
  );
}
