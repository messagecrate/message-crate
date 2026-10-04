import { TabList, TabPanel, Tabs } from "react-aria-components";
import Tab from "../../components/Tab";
import type { ServerState } from "../../lib/useServerState";
import ClaimForm from "./ClaimForm";
import CreateAccountForm from "./CreateAccountForm";
import LoginForm from "./LoginForm";

/**
 * The ways into a Message Crate, which depend on what state it is in.
 *
 * An **unclaimed** Message Crate offers one thing: creating its owner. No login,
 * because no account exists to log into, and no Create Account, because a
 * Message Crate decides who may join it only once it has an owner to decide.
 *
 * A **closed** Message Crate offers Login alone. An **open** one adds Create Account.
 *
 * The server reports which of the three it is; nothing here recombines the
 * facts behind that answer. See
 * `docs/adr/0008-the-owner-holds-no-messages.md`.
 *
 * Each panel keeps its own busy and error state, so switching tabs leaves the
 * other form's message behind.
 */
export default function LocalAuthTabs({
  serverUrl,
  serverState,
  disabled = false,
}: {
  serverUrl: string;
  serverState: ServerState;
  disabled?: boolean;
}) {
  // One thing to do, so no tab strip to choose between things.
  if (serverState === "unclaimed") {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <h2 className="mb-6 border-b border-border pb-2 text-center text-[0.875rem] font-medium text-text">
          Create Owner
        </h2>
        <ClaimForm serverUrl={serverUrl} disabled={disabled} />
      </div>
    );
  }

  if (serverState === "closed") {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <h2 className="mb-6 border-b border-border pb-2 text-center text-[0.875rem] font-medium text-text">
          Login
        </h2>
        <LoginForm serverUrl={serverUrl} disabled={disabled} />
      </div>
    );
  }

  return (
    <Tabs defaultSelectedKey="login" className="flex min-h-0 flex-1 flex-col">
      <TabList
        aria-label="Log in or create an account"
        className="relative mb-6 flex border-b border-border"
      >
        <Tab id="login" className="flex-1 text-center text-[0.875rem]">
          Login
        </Tab>
        <Tab id="create" className="flex-1 text-center text-[0.875rem]">
          Create Account
        </Tab>
      </TabList>

      <TabPanel id="login" className="flex min-h-0 flex-1 flex-col outline-none">
        <LoginForm serverUrl={serverUrl} disabled={disabled} />
      </TabPanel>
      <TabPanel id="create" className="flex min-h-0 flex-1 flex-col outline-none">
        <CreateAccountForm serverUrl={serverUrl} disabled={disabled} />
      </TabPanel>
    </Tabs>
  );
}
