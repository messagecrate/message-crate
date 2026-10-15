import { type QueryKey, useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { emptyImessagePathStats, type ImessagePathStats, isImessageMethod } from "./imessageImport";
import {
  ANDROID_SMS_NO_STATS,
  type AndroidSmsPathStats,
  isAndroidSmsSource,
} from "./importSources/androidSms";
import { probePath } from "./pathChecks";
import {
  type AndroidSmsPathsKey,
  type ImessagePathsKey,
  keys,
  type WhatsappPathsKey,
} from "./queryKeys";
import { invokeIosBackupEncrypted } from "./tauri";
import { isTauri } from "./tauriCheck";
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
 * The answer the desktop process gave for `paths`, once they stop changing
 * for {@link PATH_PROBE_DEBOUNCE_MS}, or `empty` while there is none. The
 * key is the settled paths, so each set is asked once and a set that comes
 * back is shown from the cache while it is asked again: the files can change
 * on disk between two asks, so no answer is fresh. The browser has no
 * desktop process to ask, so there it is always `empty`.
 */
function useSettledPathProbe<P extends Record<string, string> & { source: string }, S>(
  paths: P,
  {
    key,
    isMethod,
    probe,
    empty,
    keepWhileTyping,
  }: {
    key: (settled: P) => QueryKey;
    isMethod: (source: string) => boolean;
    probe: (settled: P) => Promise<S>;
    empty: S;
    /**
     * Keep the last answer for the same method on screen while a path is
     * typed. Off, any change empties the stats until the new paths are
     * checked.
     */
    keepWhileTyping: boolean;
  },
): S {
  // The fields as one string, so one timer settles them together: a source
  // change and the paths it loads never reach the key apart. Every field is
  // a string, so the round trip through JSON gives back the same fields.
  const current = JSON.stringify(paths);
  const settledText = useDebouncedValue(current, PATH_PROBE_DEBOUNCE_MS);
  const settled = useMemo(() => JSON.parse(settledText) as P, [settledText]);
  const query = useQuery({
    queryKey: key(settled),
    queryFn: async () => ({ source: settled.source, stats: await probe(settled) }),
    enabled: isTauri() && isMethod(settled.source),
    retry: false,
    staleTime: 0,
    // Another method's answer says nothing about this one's fields.
    placeholderData: (previous) =>
      keepWhileTyping && previous?.source === settled.source ? previous : undefined,
  });
  const shown = keepWhileTyping ? settled.source === paths.source : settledText === current;
  return shown && query.data ? query.data.stats : empty;
}

/**
 * What the Apple Messages fields of the Import form point at. While a path
 * is typed the last answer for the same method stays on screen, so a
 * finding does not blink out at each keystroke; another method starts empty.
 */
export function useImessagePathStats(paths: ImessagePathsKey): ImessagePathStats {
  return useSettledPathProbe<ImessagePathsKey, ImessagePathStats>(paths, {
    key: keys.desktopImessagePaths.of,
    isMethod: isImessageMethod,
    probe: async (settled) => {
      const [backup, attachment, contacts] = await Promise.all([
        probePath(settled.backupPath),
        probePath(settled.attachmentRoot),
        probePath(settled.appleContacts),
      ]);
      // Only an iPhone backup is asked whether it is encrypted.
      const encrypted =
        settled.source === "imessage-ios" && backup?.exists && backup.isDirectory
          ? await backupEncrypted(settled.backupPath.trim())
          : null;
      return {
        backup,
        attachmentRoot: attachment,
        appleContacts: contacts,
        backupEncrypted: encrypted,
      };
    },
    empty: NO_IMESSAGE_STATS,
    keepWhileTyping: true,
  });
}

/**
 * What the WhatsApp fields of the Import form point at: the backup
 * directory, the msgstore and crypt files inside it, and the contacts, media
 * and database paths. Any change empties it until the new paths are checked,
 * so a crypt file found under the old backup directory is never shown for
 * the new one.
 */
export function useWhatsappPathStats(paths: WhatsappPathsKey): WhatsappPathStats {
  return useSettledPathProbe<WhatsappPathsKey, WhatsappPathStats>(paths, {
    key: keys.desktopWhatsappPaths.of,
    isMethod: isWhatsappMethod,
    probe: async (settled) => {
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
    empty: NO_WHATSAPP_STATS,
    keepWhileTyping: false,
  });
}

/**
 * What the backup path of an Android SMS source points at: one `.xml` file
 * for SMS Backup & Restore, a directory for the others. Another source
 * starts empty, because each takes another kind of path.
 */
export function useAndroidSmsPathStats(paths: AndroidSmsPathsKey): AndroidSmsPathStats {
  return useSettledPathProbe<AndroidSmsPathsKey, AndroidSmsPathStats>(paths, {
    key: keys.desktopAndroidSmsPaths.of,
    isMethod: isAndroidSmsSource,
    probe: async (settled) => ({ backup: await probePath(settled.backupPath) }),
    empty: ANDROID_SMS_NO_STATS,
    keepWhileTyping: true,
  });
}
