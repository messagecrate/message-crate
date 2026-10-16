import { messageTags } from "../lib/messageTags";
import { useNameCollection } from "./useNameCollection";

/** Live list of message tags for the logged-in account. */
export function useMessageTags(): {
  tags: string[];
  loading: boolean;
} {
  const { names, loading } = useNameCollection(messageTags);
  return { tags: names, loading };
}
