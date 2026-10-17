import { messageTags, tagSlug } from "../lib/messageTags";
import { MESSAGE_TAG_COPY } from "../lib/namedSetCopy";
import { TagIcon } from "./icons";
import NavEntityList from "./NavEntityList";

export default function MessageTagsNav({ tags }: { tags: string[] }) {
  return (
    <NavEntityList
      names={tags}
      collection={messageTags}
      slug={tagSlug}
      icon={<TagIcon size={15} className="shrink-0" />}
      emptyIcon={<TagIcon size={15} className="shrink-0" />}
      copy={MESSAGE_TAG_COPY}
    />
  );
}
