import type { MembershipCheckState } from "../lib/membershipChecks";
import { MESSAGE_TAG_MENU_COPY } from "../lib/namedSetCopy";
import GroupsMenu from "./GroupsMenu";
import { TagIcon } from "./icons";

/** Assign or remove message tags on the selected conversations. */
export default function TagsMenu({
  allTags,
  checks,
  onToggle,
  onCreate,
  onClearAll,
  disabled = false,
}: {
  allTags: string[];
  checks: Record<string, MembershipCheckState>;
  onToggle?: (name: string) => void;
  onCreate?: (name: string) => Promise<void>;
  onClearAll?: () => void;
  disabled?: boolean;
}) {
  return (
    <GroupsMenu
      allGroups={allTags}
      checks={checks}
      onToggle={onToggle}
      onCreate={onCreate}
      onClearAll={onClearAll}
      disabled={disabled}
      copy={MESSAGE_TAG_MENU_COPY}
      icon={<TagIcon size={16} />}
      labeled={false}
    />
  );
}
