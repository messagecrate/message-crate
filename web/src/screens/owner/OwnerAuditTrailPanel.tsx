import { useState } from "react";
import { Header, ListBoxSection } from "react-aria-components";
import Select, {
  ListBoxItem,
  selectItemClassName,
  selectSectionHeaderClassName,
} from "../../components/Select";
import { formatDateTime } from "../../lib/formatDate";
import AuditTrail from "../auditTrail/AuditTrail";
import {
  type AuditTrailOf,
  auditTrailKey,
  auditTrailOf,
  useDeletedAccounts,
} from "../auditTrail/useAuditTrail";
import { sectionHint } from "../settings/storage/storageUtils";
import { useOwnerAccounts } from "./useOwnerAccounts";

const itemClassName = (state: { isFocused: boolean; isSelected: boolean }) =>
  selectItemClassName(state, "sm");

/**
 * Owner Home's Audit Trail: what each user did on this Message Crate, and
 * when, every account's entries and runs in one list, newest first.
 *
 * The account picker narrows the list to one account's entries, the ones
 * its holder reads under Settings. Below the live accounts it lists each
 * deleted account by its old username and when it was deleted, and picking
 * one narrows the list to the entries and runs that account left behind.
 */
export function OwnerAuditTrailPanel() {
  const [of, setOf] = useState<AuditTrailOf>({ kind: "all" });
  const { accounts } = useOwnerAccounts();
  const deleted = useDeletedAccounts();

  return (
    <section>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="m-0 text-text">Audit Trail</h3>
        <div className="flex items-center gap-2 text-[0.813rem] text-muted">
          <span aria-hidden="true">Account</span>
          <Select
            aria-label="Account"
            size="sm"
            className="w-[16rem]"
            selectedKey={auditTrailKey(of)}
            onSelectionChange={(key) => {
              if (key == null) return;
              setOf(auditTrailOf(String(key)));
            }}
          >
            <ListBoxItem id={auditTrailKey({ kind: "all" })} className={itemClassName}>
              Every account
            </ListBoxItem>
            {accounts.map((account) => (
              <ListBoxItem
                key={account.account_id}
                id={auditTrailKey({ kind: "account", id: account.account_id })}
                className={itemClassName}
              >
                {account.username}
              </ListBoxItem>
            ))}
            {deleted.length > 0 && (
              <ListBoxSection>
                <Header className={selectSectionHeaderClassName}>Deleted accounts</Header>
                {deleted.map((account) => {
                  const label = `${account.username}, deleted ${formatDateTime(account.deleted_at)}`;
                  return (
                    <ListBoxItem
                      key={account.id}
                      id={auditTrailKey({ kind: "deleted", id: account.id })}
                      textValue={label}
                      className={itemClassName}
                    >
                      {label}
                    </ListBoxItem>
                  );
                })}
              </ListBoxSection>
            )}
          </Select>
        </div>
      </div>
      <p className={sectionHint}>
        Logins, imports, exports and every change to an account, newest first. Entries are never
        changed or removed, and stay after an account is deleted.
      </p>
      <AuditTrail of={of} showAccount={of.kind === "all"} />
    </section>
  );
}
