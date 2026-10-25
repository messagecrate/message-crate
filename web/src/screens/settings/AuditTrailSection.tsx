import AuditTrail from "../auditTrail/AuditTrail";
import { sectionHintClass, sectionTitleClass } from "./storage/storageUtils";

/**
 * The account's own Audit Trail, under Settings: every entry about this
 * account, whoever acted, so its holder reads when the administrator changed it as
 * well as their own logins, imports and exports. Given `managedAccountId`,
 * the administrator reads the same entries for the account they opened.
 */
export function AuditTrailSection({ managedAccountId }: { managedAccountId?: number }) {
  return (
    <section>
      <h3 className={sectionTitleClass}>Audit Trail</h3>
      <p className={sectionHintClass}>
        What was done with this account and when: logins, imports, exports, and changes made by the
        account holder or the administrator.
      </p>
      <AuditTrail
        of={
          managedAccountId === undefined
            ? { kind: "own" }
            : { kind: "account", id: managedAccountId }
        }
        showAccount={false}
      />
    </section>
  );
}
