import AuditTrail from "../auditTrail/AuditTrail";
import { sectionHintClass, sectionTitleClass } from "./storage/storageUtils";

/**
 * The account's own Audit Trail, under Settings: every entry about this
 * account, whoever acted, so its holder reads when the owner changed it as
 * well as their own logins, imports and exports. Given `managedAccountId`,
 * the owner reads the same entries for the account they opened.
 */
export function AuditTrailSection({ managedAccountId }: { managedAccountId?: number }) {
  return (
    <section>
      <h3 className={sectionTitleClass}>Audit Trail</h3>
      <p className={sectionHintClass}>
        What was done with this account and when: logins, imports, exports, and changes made by the
        account holder or the owner.
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
