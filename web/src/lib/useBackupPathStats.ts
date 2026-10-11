import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import {
  emptyImessagePathStats,
  type ImessagePathStats,
  imessageStatsForMethod,
  isImessageMethod,
} from "./imessageImport";
import { probePath } from "./pathChecks";
import { keys } from "./queryKeys";
import { invokeIosBackupEncrypted } from "./tauri";
import { isTauri } from "./tauri-check";
import { useDebouncedValue } from "./useDebouncedValue";
import {
  emptyWhatsappPathStats,
  isWhatsappMethod,
  WHATSAPP_CRYPT_NAMES,
  type WhatsappPathStats,
} from "./whatsappImport";

/** How long the Import form waits after the last keystroke in a path field before it checks the paths. */
export const PATH_PROBE_DEBOUNCE_MS = 200;

const NO_IMESSAGE_STATS = emptyImessagePathStats();
const NO_WHATSAPP_STATS = emptyWhatsappPathStats();

/** Whether the iPhone backup directory at `root` is encrypted; null when the desktop process cannot say. */
async function backupEncrypted(root: string): Promise<boolean | null> {
  try {
    return await invokeIosBackupEncrypted(root);
  } catch {
    return null;
  }
}

/**
 * What the Apple Messages fields of the Import form point at, read from the
 * desktop process once the fields stop changing for
 * {@link PATH_PROBE_DEBOUNCE_MS}. While a path is typed the last answer for
 * the same method stays on screen; another method starts empty. The browser
 * has no desktop process to ask, so there it stays empty.
 */
export function useImessagePathStats(
  source: string,
  backupPath: string,
  attachmentRoot: string,
  appleContacts: string,
): ImessagePathStats {
  const current = useMemo(
    () => ({ source, backupPath, attachmentRoot, appleContacts }),
    [source, backupPath, attachmentRoot, appleContacts],
  );
  const settled = useDebouncedValue(current, PATH_PROBE_DEBOUNCE_MS);
  const method = settled.source;
  const query = useQuery({
    queryKey: keys.desktopImessagePaths(
      method,
      settled.backupPath,
      settled.attachmentRoot,
      settled.appleContacts,
    ),
    queryFn: async () => {
      if (!isImessageMethod(method)) return NO_IMESSAGE_STATS;
      const [backup, attachment, contacts] = await Promise.all([
        probePath(settled.backupPath),
        probePath(settled.attachmentRoot),
        probePath(settled.appleContacts),
      ]);
      const encrypted =
        method === "imessage-ios" && backup?.exists && backup.isDirectory
          ? await backupEncrypted(settled.backupPath.trim())
          : null;
      return imessageStatsForMethod(method, {
        backup,
        attachmentRoot: attachment,
        appleContacts: contacts,
        backupEncrypted: encrypted,
      });
    },
    enabled: isTauri() && isImessageMethod(method),
    retry: false,
    // The previous paths' answer, kept while the new ones are checked, but
    // only for the same method: another method's answer says nothing here.
    placeholderData: (previous, previousQuery) =>
      previousQuery?.queryKey[2] === method ? previous : undefined,
  });
  return method === source && query.data ? query.data : NO_IMESSAGE_STATS;
}

/**
 * What the WhatsApp fields of the Import form point at, read from the
 * desktop process once the fields stop changing for
 * {@link PATH_PROBE_DEBOUNCE_MS}: the backup directory, the msgstore and
 * crypt files inside it, and the contacts, media and database paths. Any
 * change empties it until the new paths are checked, so a file found under
 * the old backup directory is never reported for the new one. The browser
 * has no desktop process to ask, so there it stays empty.
 */
export function useWhatsappPathStats(
  source: string,
  backupPath: string,
  contactsDb: string,
  media: string,
  db: string,
): WhatsappPathStats {
  const current = useMemo(
    () => ({ source, backupPath, contactsDb, media, db }),
    [source, backupPath, contactsDb, media, db],
  );
  const settled = useDebouncedValue(current, PATH_PROBE_DEBOUNCE_MS);
  const query = useQuery({
    queryKey: keys.desktopWhatsappPaths(
      settled.source,
      settled.backupPath,
      settled.contactsDb,
      settled.media,
      settled.db,
    ),
    queryFn: async (): Promise<WhatsappPathStats> => {
      const root = settled.backupPath.trim();
      const [backup, contactsStat, mediaStat, dbStat, msgstore, cryptHits] = await Promise.all([
        probePath(settled.backupPath),
        probePath(settled.contactsDb),
        probePath(settled.media),
        probePath(settled.db),
        probePath(root ? `${root}/msgstore.db` : ""),
        Promise.all(
          WHATSAPP_CRYPT_NAMES.map(async (name) => {
            const stat = await probePath(root ? `${root}/${name}` : "");
            return stat?.exists && stat.isFile ? name : null;
          }),
        ),
      ]);
      const encrypted =
        settled.source === "whatsapp-ios" && root !== "" ? await backupEncrypted(root) : null;
      return {
        backup,
        contactsDb: contactsStat,
        media: mediaStat,
        db: dbStat,
        hasMsgstoreDb: Boolean(msgstore?.exists && msgstore.isFile),
        cryptName: cryptHits.find((name) => name !== null) ?? null,
        backupEncrypted: encrypted,
      };
    },
    enabled: isTauri() && isWhatsappMethod(settled.source),
    retry: false,
  });
  return settled === current && query.data ? query.data : NO_WHATSAPP_STATS;
}
