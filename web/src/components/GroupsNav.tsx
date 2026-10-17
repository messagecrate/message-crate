import { contactGroups, groupSlug } from "../lib/contactGroups";
import { CONTACT_GROUP_COPY } from "../lib/namedSetCopy";
import { PeopleGroupIcon, PersonIcon } from "./icons";
import NavEntityList from "./NavEntityList";

export default function GroupsNav({ groups }: { groups: string[] }) {
  return (
    <NavEntityList
      names={groups}
      collection={contactGroups}
      slug={groupSlug}
      icon={<PeopleGroupIcon size={15} className="shrink-0" />}
      emptyIcon={<PersonIcon size={15} className="shrink-0" />}
      copy={CONTACT_GROUP_COPY}
    />
  );
}
