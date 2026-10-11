import { useNavigate } from "react-router-dom";
import Button from "../../components/Button";
import { GearIcon } from "../../components/icons";
import NavGlyphButton from "../../components/NavGlyphButton";
import ScrollingTableCard from "../../components/ScrollingTableCard";
import { formatDateTime } from "../../lib/formatDate";
import { tdClass, tdMutedClass } from "../settings/apiTokensUtils";
import { rowStripeClass, thClass, thSeparatorClass } from "./ownerTableStyles";
import { type ManagedAccount, useOwnerAccounts } from "./useOwnerAccounts";

/** Columns the table has, which the "no match" row spans. */
const COLUMN_COUNT = 4;

/** What the Status column reads for one account. */
function statusLabel(account: ManagedAccount): string {
  if (account.is_owner) return "Owner";
  return account.disabled ? "Disabled" : "Active";
}

/**
 * Whether the Status column marks the account as having no password. Anyone
 * who knows its username logs in with an empty password. The Demo Account is
 * left out: it has no password by design (ADR 0016).
 */
function opensWithoutPassword(account: ManagedAccount): boolean {
  return !account.has_password && !account.is_demo;
}

/** Whether the search bar's words are in the account's username or preferred name. */
function matches(account: ManagedAccount, needle: string): boolean {
  return (
    account.username.toLowerCase().includes(needle) ||
    (account.preferred_name ?? "").toLowerCase().includes(needle)
  );
}

/**
 * The accounts of this Message Crate, the owner's own first.
 *
 * A row carries a username, a preferred name, a status and the last login.
 * The status of an account with no password says so under it, so the owner
 * sees every account open to an empty password.
 * The gear at the left of a row, shown while the pointer is in the row, opens
 * the account's Settings, which is where the rest is: the app it connects with
 * under Profile, what it holds under Storage, and its password, status and
 * permissions under Account. The table sets nothing; it shows each status so a
 * disabled account stands out. Add account opens the same Settings for an
 * account that does not exist yet.
 */
export function OwnerAccountsPanel({ filter = "" }: { filter?: string }) {
  const navigate = useNavigate();
  const { accounts, loading, loadError } = useOwnerAccounts();

  if (loading) return <p className="text-[0.875rem] text-muted">Loading accounts…</p>;
  if (loadError) return <p className="text-[0.875rem] text-danger">{loadError}</p>;

  // The header search bar narrows the table by username or preferred name.
  const needle = filter.trim().toLowerCase();
  const shown = needle ? accounts.filter((a) => matches(a, needle)) : accounts;

  return (
    <section>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="m-0 text-text">User Accounts</h3>
        {/* A new account starts where an existing one is changed: its Settings. */}
        <Button variant="secondary" size="xs" onClick={() => navigate("/owner/accounts/new")}>
          Add account
        </Button>
      </div>

      <ScrollingTableCard className="mt-4" cardClassName="rounded-xl bg-elevated">
        <table className="w-full border-collapse">
          <thead>
            <tr>
              {/* The gear column has no heading; each gear is labelled with its account. */}
              <td className="w-6 py-2 pl-3" />
              <th className={thClass}>User</th>
              <th className={`${thClass} ${thSeparatorClass}`}>Status</th>
              <th className={`${thClass} ${thSeparatorClass}`}>Last login</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((account) => {
              const preferredName = account.preferred_name?.trim() ?? "";
              return (
                <tr
                  key={account.account_id}
                  className={`group border-t border-border ${rowStripeClass}`}
                >
                  <td className="w-6 py-2 pl-3 align-middle">
                    <NavGlyphButton
                      aria-label={`Settings for ${account.username}`}
                      onClick={() => navigate(`/owner/accounts/${account.account_id}`)}
                      className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                    >
                      <GearIcon size={15} />
                    </NavGlyphButton>
                  </td>
                  <td className={`${tdClass} whitespace-nowrap`}>
                    <div className="font-semibold">{account.username}</div>
                    {preferredName ? (
                      <div className="text-[0.75rem] text-muted">{preferredName}</div>
                    ) : null}
                  </td>
                  <td className={account.disabled ? tdClass : tdMutedClass}>
                    <div>{statusLabel(account)}</div>
                    {opensWithoutPassword(account) ? (
                      <div className="text-[0.75rem] text-warn-soft-text">No password</div>
                    ) : null}
                  </td>
                  <td className={`${tdMutedClass} whitespace-nowrap`}>
                    {account.last_login_at ? formatDateTime(account.last_login_at) : "Never"}
                  </td>
                </tr>
              );
            })}
            {shown.length === 0 && needle ? (
              <tr className="border-t border-border">
                <td className={tdMutedClass} colSpan={COLUMN_COUNT}>
                  No account matches “{filter.trim()}”.
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </ScrollingTableCard>
    </section>
  );
}
