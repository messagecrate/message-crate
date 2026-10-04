import { useState } from "react";
import { Header, ListBoxSection } from "react-aria-components";
import Select, {
  ListBoxItem,
  selectItemClassName,
  selectSectionHeaderClassName,
} from "../../components/Select";
import AuditTrail from "../auditTrail/AuditTrail";
import { type AuditTrailOf, useDeletedAccountUsernames } from "../auditTrail/useAuditTrail";
import { sectionHint } from "../settings/storage/storageUtils";
import { useOwnerAccounts } from "./useOwnerAccounts";

/** The picker's key for the full list, beside each account's id. */
const EVERY_ACCOUNT = "all";

/**
 * The start of a deleted account's key in the picker, before its username.
 * A live account's key is its id, all digits, so the two never meet.
 */
const DELETED = "deleted:";

const itemClassName = (state: { isFocused: boolean; isSelected: boolean }) =>
  selectItemClassName(state, "sm");

/** The picker's key for whose trail is shown. */
function pickerKey(of: AuditTrailOf): string {
  switch (of.kind) {
    case "account":
      return String(of.id);
    case "deleted":
      return `${DELETED}${of.username}`;
    default:
      return EVERY_ACCOUNT;
  }
}

/** Whose trail a picker key names. */
function fromPickerKey(key: string): AuditTrailOf {
  if (key === EVERY_ACCOUNT) return { kind: "all" };
  if (key.startsWith(DELETED)) return { kind: "deleted", username: key.slice(DELETED.length) };
  return { kind: "account", id: Number(key) };
}

/**
 * Owner Home's Audit Trail: what each user did on this Message Crate, and
 * when, every account's entries and runs in one list, newest first.
 *
 * The account picker narrows the list to one account's entries, the ones
 * its holder reads under Settings. Below the live accounts it lists the
 * deleted ones by their old usernames, and picking one narrows the list to
 * the entries and runs that account left behind.
 */
export function OwnerAuditTrailPanel() {
  const [of, setOf] = useState<AuditTrailOf>({ kind: "all" });
  const { accounts } = useOwnerAccounts();
  const deleted = useDeletedAccountUsernames();

  return (
    <section>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="m-0 text-text">Audit Trail</h3>
        <div className="flex items-center gap-2 text-[0.813rem] text-muted">
          <span aria-hidden="true">Account</span>
          <Select
            aria-label="Account"
            size="sm"
            className="w-[12rem]"
            selectedKey={pickerKey(of)}
            onSelectionChange={(key) => {
              if (key == null) return;
              setOf(fromPickerKey(String(key)));
            }}
          >
            <ListBoxItem id={EVERY_ACCOUNT} className={itemClassName}>
              Every account
            </ListBoxItem>
            {accounts.map((account) => (
              <ListBoxItem
                key={account.account_id}
                id={String(account.account_id)}
                className={itemClassName}
              >
                {account.username}
              </ListBoxItem>
            ))}
            {deleted.length > 0 && (
              <ListBoxSection>
                <Header className={selectSectionHeaderClassName}>Deleted accounts</Header>
                {deleted.map((username) => (
                  <ListBoxItem
                    key={`${DELETED}${username}`}
                    id={`${DELETED}${username}`}
                    textValue={`${username} (deleted)`}
                    className={itemClassName}
                  >
                    {username} (deleted)
                  </ListBoxItem>
                ))}
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
