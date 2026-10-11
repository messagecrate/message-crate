import { useEffect, useRef, useState } from "react";
import { phoneNumbers, profileAddresses } from "../lib/account";
import { apiErrorMessage } from "../lib/apiErrorMessage";
import { attachmentChoicesOf, DEFAULT_ATTACHMENT_CHOICES } from "../lib/attachmentChoices";
import { type IdentityType, identityOnProfile, parseSourceIdentities } from "../lib/backupIdentity";
import { getDeviceId } from "../lib/deviceId";
import {
  emptyImessagePathStats,
  IMESSAGE_DEFAULT_METHOD,
  imessageStatsForMethod,
  isImessageMethod,
  macMessagesDbPath,
  shouldPrefillMacMessagesDb,
} from "../lib/imessageImport";
import { type ActiveImportRun, getActiveImportRun } from "../lib/importRun";
import { importSourceById, importSourceFor } from "../lib/importSources";
import { splitEmails } from "../lib/importSources/androidSms";
import { serverService } from "../lib/offeredService";
import { probePath } from "../lib/pathChecks";
import { usePhoneCountries } from "../lib/phoneCountries";
import { keys } from "../lib/queryKeys";
import { useRouteCache, useRouteQuery } from "../lib/routeQuery";
import { unmatchedIdentities } from "../lib/serverApi";
import {
  getImporterPath,
  getRememberImporterPaths,
  type ImporterExtraField,
  loadRememberedImportPaths,
  setImporterExtraPath,
  setImporterPath,
} from "../lib/system-settings";
import { invokeHomeDir, invokeIosBackupEncrypted, invokePathStat } from "../lib/tauri";
import { isTauri } from "../lib/tauri-check";
import { useTimeZone } from "../lib/timeZone";
import type { AttachmentChoices } from "../lib/types";
import {
  useAccountProfile,
  useFetchAccountProfile,
  useUpdateAccountProfile,
} from "../lib/useAccountProfile";
import {
  emptyWhatsappPathStats,
  isWhatsappMethod,
  WHATSAPP_CRYPT_NAMES,
} from "../lib/whatsappImport";
import BackupIdentityList from "./import/BackupIdentityList";
import BackupIdentityStopScreen from "./import/BackupIdentityStopScreen";
import { restoreFormFromSnapshot, snapshotSecret } from "./import/formSnapshot";
import ImportFormFields from "./import/ImportFormFields";
import ImportRunView from "./import/ImportRunView";
import { isReviewPhase } from "./import/importRunStore";
import ResumeImportPanel from "./import/ResumeImportPanel";
import RunDirDeleteFailureNotice from "./import/RunDirDeleteFailureNotice";
import {
  checkSourceFingerprint,
  type DirectoryCheck,
  type FingerprintCheck,
  type ResumeDecision,
  resumeDecisionFor,
  resumeReadsBackup,
} from "./import/resumeDecision";
import { parseStoredStagingSummary } from "./import/run/stagingSummary";
import { useImportJob } from "./import/useImportJob";

const DEFAULT_SOURCE = IMESSAGE_DEFAULT_METHOD;
const PATH_PROBE_DEBOUNCE_MS = 200;
/** The server's own cap on one `/v1/contacts/unmatched-identities` request (`MAX_MATCH_IDENTIFIERS`,
 * `crates/server/server/src/contacts_api.rs`) — the client batches to it rather than
 * discovering the limit from a 422. */
const MAX_MATCH_IDENTIFIERS = 500;

/** Stands in for a staging summary's identifiers before there is one, as one stable value. */
const NO_IDENTIFIERS: readonly string[] = [];

/** Nothing to decide -- the form renders. The one spelling of "no resume". */
const NO_RESUME: ResumeDecision = { kind: "none", run: null };

/**
 * Whether a run's directory is still on disk. A check that fails outright,
 * or a directory the operating system will not describe, is "unknown", not
 * "missing": neither says anything about whether the directory is there,
 * and reading it as gone would offer to discard staged work that may well
 * still be there.
 */
async function runDirectoryCheck(runDir: string): Promise<DirectoryCheck> {
  const stat = await probePath(runDir);
  if (stat === null || stat.unreadable) return "unknown";
  return stat.exists && stat.isDirectory ? "present" : "missing";
}

export default function ImportScreen() {
  const fetchAccountProfile = useFetchAccountProfile();
  const cache = useRouteCache();
  const {
    phase,
    steps,
    running,
    form,
    summaryView,
    runDir,
    importRunId,
    stagingSummary,
    mediaSummary,
    mediaFailedCount,
    mediaToolsMissing,
    mediaPartiallyRan,
    resumeError,
    reviewError,
    sourceIdentities,
    computingSummary,
    completionText,
    startImport,
    approve,
    cancelRun,
    resumeAtReview,
    cancel,
    returnToForm,
    continueAfterIdentityStop,
    cancelIdentityStop,
    runDirDeleteFailure,
    discardRun,
    dismissRunDirDeleteFailure,
  } = useImportJob();
  /** Which review the run is waiting at, or null while it is not waiting. */
  const reviewWaiting = isReviewPhase(phase)
    ? phase === "staging_review"
      ? "staging"
      : "media"
    : null;

  /**
   * How many of the staged contact identifiers this account has no contact
   * for, asked while a review is shown, batched at the server's own cap so a
   * large import doesn't send an oversized request. A failure is shown on the
   * review and does not block it: the split into existing and new contacts
   * helps the person decide, and approving does not depend on it.
   */
  const contactIdentifiers = stagingSummary?.contactIdentifiers ?? NO_IDENTIFIERS;
  const unknownContactsQuery = useRouteQuery(
    keys.contacts.unmatchedCount(contactIdentifiers),
    async (signal) => {
      let total = 0;
      for (let i = 0; i < contactIdentifiers.length; i += MAX_MATCH_IDENTIFIERS) {
        const batch = contactIdentifiers.slice(i, i + MAX_MATCH_IDENTIFIERS);
        const res = await unmatchedIdentities({ identifiers: batch }, { signal });
        total += res.items.length;
      }
      return total;
    },
    { enabled: isReviewPhase(phase) && stagingSummary != null },
  );
  const unknownContacts = unknownContactsQuery.data ?? null;
  const unknownContactsError = unknownContactsQuery.error
    ? apiErrorMessage(unknownContactsQuery.error, "The server didn't answer.")
    : null;

  const { profile } = useAccountProfile();
  const updateProfile = useUpdateAccountProfile();
  const identityProfile = profile ? profileAddresses(profile) : null;
  const identityAddBusy = updateProfile.isPending;
  const [identityAddError, setIdentityAddError] = useState<string | null>(null);

  /** Link one backup address onto the profile; the marks re-derive from the
   * updated profile, so a claimed address resolves a mismatch in place.
   * Never rejects: a failed add (or a 200 that didn't actually add it) is
   * caught here and turned into `identityAddError` rather than an unhandled
   * rejection through the fire-and-forget `void onAdd(...)` call in
   * BackupIdentityList/BackupIdentityStopScreen. */
  const addIdentityToProfile = async (value: string, type: IdentityType): Promise<void> => {
    setIdentityAddError(null);
    try {
      const updated = await updateProfile.mutateAsync({
        identities: [{ address: value, service: serverService(type) }],
      });
      if (!identityOnProfile(value, profileAddresses(updated))) {
        throw new Error("no-op add");
      }
    } catch {
      setIdentityAddError("The server didn't add that address.");
    }
  };

  const [source, setSource] = useState(DEFAULT_SOURCE);
  const [backupPath, setBackupPath] = useState(() =>
    getRememberImporterPaths() ? getImporterPath(DEFAULT_SOURCE) : "",
  );
  const [attachmentRoot, setAttachmentRoot] = useState("");
  const [appleContacts, setAppleContacts] = useState("");
  const [pathStats, setPathStats] = useState(emptyImessagePathStats);
  const [whatsappKey, setWhatsappKey] = useState("");
  const [showWhatsappKey, setShowWhatsappKey] = useState(false);
  const [whatsappWa, setWhatsappWa] = useState("");
  const [whatsappMedia, setWhatsappMedia] = useState("");
  const [whatsappDb, setWhatsappDb] = useState("");
  const [isBusinessApp, setIsBusinessApp] = useState(false);
  /** The holder's WhatsApp number; seeded from the profile's first phone. */
  const [whatsappOwnerPhone, setWhatsappOwnerPhone] = useState("");
  const [whatsappStats, setWhatsappStats] = useState(emptyWhatsappPathStats);
  const [backupPassword, setBackupPassword] = useState("");
  const [showBackupPassword, setShowBackupPassword] = useState(false);
  const [attachments, setAttachments] = useState<AttachmentChoices>(DEFAULT_ATTACHMENT_CHOICES);
  const [ownerPhones, setOwnerPhones] = useState<string[]>([]);
  /** Owner email addresses as typed; split into a list when the import starts. */
  const [ownerEmails, setOwnerEmails] = useState("");
  const [formatOpen, setFormatOpen] = useState(true);
  const [processingOpen, setProcessingOpen] = useState(false);
  const [obfuscate, setObfuscate] = useState(false);
  const [phoneCountry, setPhoneCountry] = useState("");
  const { countries: phoneCountries } = usePhoneCountries();
  /** The zone iMazing dates are read in. The account's zone until the person
   * picks another under Processing Options; null means "the account's". */
  const accountTimeZone = useTimeZone();
  const [timeZoneOverride, setTimeZoneOverride] = useState<string | null>(null);
  const timeZone = timeZoneOverride ?? accountTimeZone;
  /** Profile phones after SBR fetch; empty until ready (or after a failed fetch). */
  const [profilePhones, setProfilePhones] = useState<string[]>([]);
  const [profilePhonesReady, setProfilePhonesReady] = useState(false);
  const [profilePhonesError, setProfilePhonesError] = useState(false);
  const ownerPhonesSeededRef = useRef(false);
  const ownerEmailsSeededRef = useRef(false);
  const whatsappOwnerPhoneSeededRef = useRef(false);
  // The method last picked for each source with several, by source id, so
  // picking the source again returns to it.
  const lastMethodRef = useRef<Record<string, string>>({});
  const sourceChangeGenRef = useRef(0);

  const [resume, setResume] = useState<ResumeDecision>(NO_RESUME);
  const [resumeChecked, setResumeChecked] = useState(false);
  const discardingRef = useRef(false);
  const resumingRef = useRef(false);

  /**
   * Ask the server which Import Run is open, on mount and on every return to the
   * form.
   *
   * Re-checking matters because the server can hold a run the screen has
   * already forgotten: a swallowed final /complete, or a restart whose
   * discard failed before the create 409'd. Without it, Back lands on a
   * blank form whose Import button 409s until the route is remounted.
   *
   * The answer is stored under `keys.imports.running`, the entry the
   * sidebar's Import badge reads, so every return to the form refreshes the
   * badge too and the two cannot disagree.
   *
   * Nothing here writes `phase` or `cache`, so this cannot loop; the early
   * return keeps it from running against a run an import is currently
   * using.
   */
  useEffect(() => {
    if (phase !== "form") return;
    let cancelled = false;
    void (async () => {
      try {
        const run = await cache.fetch(keys.imports.running, (signal) => getActiveImportRun(signal));
        const directory = run?.run_dir ? await runDirectoryCheck(run.run_dir) : "missing";
        // Only a resume of the copy consults this; every later stage works
        // from the staged directory rather than the backup. The desktop
        // `PathStat`, not `probePath`'s `ImportPathStat`: the comparison needs
        // the size and modified time, which `ImportPathStat` leaves out. A
        // check that fails says nothing about the backup, so it is
        // "unknown", never a backup that is gone.
        const stored = run?.source_fingerprint ?? null;
        const fingerprint: FingerprintCheck = stored?.path
          ? await invokePathStat(stored.path).then(
              (stat) => checkSourceFingerprint(stored, stat),
              () => "unknown" as const,
            )
          : checkSourceFingerprint(stored, null);
        // A resume or discard that started while this was in flight owns
        // the decision -- a stale answer must not put the panel back.
        if (!cancelled && !resumingRef.current && !discardingRef.current) {
          setResume(
            resumeDecisionFor({
              run,
              deviceId: getDeviceId(),
              directory,
              fingerprint,
            }),
          );
        }
      } catch {
        // A server that cannot answer is not a reason to block the form.
        if (!cancelled && !resumingRef.current && !discardingRef.current) setResume(NO_RESUME);
      } finally {
        // Only the first check decides what renders; a later one must not
        // blank the form while it runs.
        if (!cancelled) setResumeChecked(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [phase, cache]);

  /** Populate the visible form from a resumed or restarted run's settings. */
  function applyRestoredFormState(restored: ReturnType<typeof restoreFormFromSnapshot>): void {
    if (!restored) return;
    setSource(restored.source);
    setBackupPath(restored.backupPath);
    setBackupPassword("");
    setAttachmentRoot(restored.attachmentRoot);
    setAppleContacts(restored.appleContacts);
    setWhatsappKey("");
    setWhatsappWa(restored.whatsappWa);
    setWhatsappMedia(restored.whatsappMedia);
    setWhatsappDb(restored.whatsappDb);
    setIsBusinessApp(restored.isBusinessApp);
    whatsappOwnerPhoneSeededRef.current = true;
    setWhatsappOwnerPhone(restored.whatsappOwnerPhone);
    setAttachments(attachmentChoicesOf(restored));
    // Restoring settings counts as seeding: the SBR profile-phones effect
    // must not overwrite what was just restored.
    ownerPhonesSeededRef.current = true;
    setOwnerPhones(restored.ownerPhones);
    ownerEmailsSeededRef.current = true;
    setOwnerEmails(restored.ownerEmails.join(", "));
    setObfuscate(restored.obfuscate);
    setPhoneCountry(restored.phoneCountry);
    setTimeZoneOverride(restored.timeZone);
  }

  /**
   * Discard the run the panel offers (`discardRun`): it closes with the
   * Import Errors its record holds, and its directory goes, so it must not
   * orphan a multi-GB directory. A directory that could not be deleted is shown
   * above the form (`runDirDeleteFailure`), never dropped without a word.
   * A run staged on another device is never touched on disk here,
   * because its files are staged on that device. The check is the same
   * `device_id` check `resumeDecisionFor` uses to route to `other_device`,
   * and a run with no recorded device counts as this install's there too.
   */
  async function discardOfferedRun(run: ActiveImportRun): Promise<void> {
    const thisDevice = !run.device_id || run.device_id === getDeviceId();
    await discardRun(run.id, thisDevice ? run.run_dir : null);
  }

  async function handleDiscardResume(): Promise<void> {
    const run = resume.run;
    if (!run || discardingRef.current || resumingRef.current) return;
    discardingRef.current = true;
    try {
      // The panel drops to the form either way; if the run is still
      // live server-side, the next visit shows it again.
      await discardOfferedRun(run);
    } finally {
      discardingRef.current = false;
      setResume(NO_RESUME);
      // The run is gone, or the server still has it; either way the badge
      // asks again rather than keep saying "waiting".
      cache.invalidateAccount();
    }
  }

  // The password or key the stored run was started with, when this resume
  // runs extract again. The snapshot records only that one was given, so the
  // panel asks for it. A resume into Upload reads no backup and asks nothing.
  const resumeSecret =
    resume.run && resumeReadsBackup(resume.kind) ? snapshotSecret(resume.run.form) : null;

  async function handleResumeAction(typedSecret: string): Promise<void> {
    if (resume.kind === "none" || !resume.run) return;
    // The panel holds its button until the field is filled; this keeps an
    // extract with an empty password from starting by any other route.
    if (resumeSecret && typedSecret.trim() === "") return;
    // The panel deliberately stays mounted across the discard round trip
    // below, so without this a second click would run two discards, two
    // startImport calls (the second 409s), and two extracts racing one set
    // of screen state.
    if (resumingRef.current || discardingRef.current) return;
    resumingRef.current = true;
    try {
      const run = resume.run;
      const storedForm = restoreFormFromSnapshot(run.form);
      if (!storedForm) {
        // The run directory is present -- the decision only reached here
        // because it is -- so directory_missing's copy would be false. This
        // kind exists solely for this screen to construct.
        setResume({ kind: "settings_unreadable", run });
        return;
      }
      applyRestoredFormState(storedForm);
      // The typed secret goes to this one run's extract and nowhere else:
      // not into the visible form's state, and never into a snapshot.
      const restoredForm = resumeSecret
        ? { ...storedForm, [resumeSecret]: typedSecret }
        : storedForm;

      if (resume.kind === "resume_upload") {
        if (!run.run_dir) return; // resumeDecisionFor guarantees this; defensive only.
        setResume(NO_RESUME);
        await startImport(restoredForm, {
          runId: run.id,
          runDir: run.run_dir,
          // Without this, a resumed Upload has no plan to diff its expected
          // omissions against, which demotes an honest `completed` verdict
          // to `completed_with_issues` for exactly the interrupted-and-
          // resumed case. Undefined when the stored summary is missing or
          // unparsable — startImport/runUpload already tolerate that.
          approved: parseStoredStagingSummary(run.summary),
        });
        return;
      }

      if (resume.kind === "resume_review" || resume.kind === "resume_media") {
        if (!run.run_dir) return; // resumeDecisionFor guarantees this; defensive only.
        setResume(NO_RESUME);
        await resumeAtReview(run, restoredForm);
        return;
      }

      if (resume.kind === "resume_write") {
        if (!run.run_dir) return; // resumeDecisionFor guarantees this; defensive only.
        setResume(NO_RESUME);
        await startImport(restoredForm, undefined, {
          runId: run.id,
          runDir: run.run_dir,
          // The write is resumed rather than re-probed, so the Staging Review's identity
          // section has to come from what was recorded on the run at
          // creation rather than a fresh read of the backup.
          identities: parseSourceIdentities(run.source_identities),
        });
        return;
      }

      // Restart: a fresh extract writes into a new run directory, and the
      // server allows only one running Import Run per account, so give up the old
      // one before starting the new run. setResume stays put until right
      // before startImport, so the panel (not a blank form) covers the
      // discard round trip. The old directory goes with the run: nothing
      // will ever reach it again, and it can be multiple gigabytes. A failed
      // delete stays on screen through the new run.
      // If the server is unreachable, the create call below surfaces its
      // own error the same as any other failed import start.
      await discardOfferedRun(run);
      cache.invalidateAccount();
      setResume(NO_RESUME);
      await startImport(restoredForm);
    } finally {
      resumingRef.current = false;
    }
  }

  // The profile's phones seed the owner fields: the owner phone list of a
  // source that asks for it, and the WhatsApp number of a source that asks
  // for that instead.
  useEffect(() => {
    const descriptor = importSourceFor(source);
    if (!descriptor.asksOwnerPhones && !descriptor.asksWhatsappOwnerPhone) {
      setProfilePhones([]);
      setProfilePhonesReady(false);
      setProfilePhonesError(false);
      ownerPhonesSeededRef.current = false;
      ownerEmailsSeededRef.current = false;
      whatsappOwnerPhoneSeededRef.current = false;
      return;
    }
    const wantsEmails = descriptor.asksOwnerEmails;
    let cancelled = false;
    setProfilePhonesReady(false);
    setProfilePhonesError(false);
    void (async () => {
      try {
        const profile = await fetchAccountProfile();
        if (cancelled) return;
        if (!profile) throw new Error("profile unavailable");
        const phones = phoneNumbers(profile);
        setProfilePhones(phones);
        setProfilePhonesError(false);
        setProfilePhonesReady(true);
        if (descriptor.asksWhatsappOwnerPhone) {
          const [first] = phones;
          if (first === undefined || whatsappOwnerPhoneSeededRef.current) return;
          setWhatsappOwnerPhone((current) => {
            if (current.trim().length > 0) return current;
            whatsappOwnerPhoneSeededRef.current = true;
            return first;
          });
          return;
        }
        if (wantsEmails && profile.emails.length > 0 && !ownerEmailsSeededRef.current) {
          setOwnerEmails((current) => {
            if (current.trim().length > 0) return current;
            ownerEmailsSeededRef.current = true;
            return profile.emails.join(", ");
          });
        }
        if (phones.length === 0 || ownerPhonesSeededRef.current) return;
        setOwnerPhones((current) => {
          if (current.length > 0) return current;
          ownerPhonesSeededRef.current = true;
          return phones;
        });
      } catch {
        if (!cancelled) {
          setProfilePhones([]);
          setProfilePhonesError(true);
          setProfilePhonesReady(true);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [source, fetchAccountProfile]);

  useEffect(() => {
    return () => {
      sourceChangeGenRef.current += 1;
    };
  }, []);

  useEffect(() => {
    if (!isTauri() || !isImessageMethod(source)) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void (async () => {
        const [backup, attachment, contacts] = await Promise.all([
          probePath(backupPath),
          probePath(attachmentRoot),
          probePath(appleContacts),
        ]);
        let backupEncrypted: boolean | null = null;
        if (source === "imessage-ios" && backup?.exists && backup.isDirectory) {
          try {
            backupEncrypted = await invokeIosBackupEncrypted(backupPath.trim());
          } catch {
            backupEncrypted = null;
          }
        }
        if (cancelled) return;
        const next = {
          backup,
          attachmentRoot: attachment,
          appleContacts: contacts,
          backupEncrypted,
        };
        setPathStats(imessageStatsForMethod(source, next));
      })();
    }, PATH_PROBE_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [source, backupPath, attachmentRoot, appleContacts]);

  useEffect(() => {
    if (!isTauri() || !isWhatsappMethod(source)) return;
    setWhatsappStats(emptyWhatsappPathStats());
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void (async () => {
        const root = backupPath.trim();
        const [backup, contactsDb, media, db, msgstore, cryptHits] = await Promise.all([
          probePath(backupPath),
          probePath(whatsappWa),
          probePath(whatsappMedia),
          probePath(whatsappDb),
          probePath(root ? `${root}/msgstore.db` : ""),
          Promise.all(
            WHATSAPP_CRYPT_NAMES.map(async (name) => {
              const stat = await probePath(root ? `${root}/${name}` : "");
              return stat?.exists && stat.isFile ? name : null;
            }),
          ),
        ]);
        let backupEncrypted: boolean | null = null;
        if (source === "whatsapp-ios" && root !== "") {
          try {
            backupEncrypted = await invokeIosBackupEncrypted(root);
          } catch {
            backupEncrypted = null;
          }
        }
        if (cancelled) return;
        const cryptName = cryptHits.find((name) => name !== null) ?? null;
        setWhatsappStats({
          backup,
          contactsDb,
          media,
          db,
          hasMsgstoreDb: Boolean(msgstore?.exists && msgstore.isFile),
          cryptName,
          backupEncrypted,
        });
      })();
    }, PATH_PROBE_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [source, backupPath, whatsappWa, whatsappMedia, whatsappDb]);

  function applyRememberedPaths(nextSource: string): string {
    const loaded = loadRememberedImportPaths(nextSource);
    const remembered = importSourceFor(nextSource).rememberedPaths;
    const pathOf = (field: ImporterExtraField) => (remembered.includes(field) ? loaded[field] : "");
    setBackupPath(loaded.backupPath);
    setAttachmentRoot(pathOf("attachmentRoot"));
    setAppleContacts(pathOf("appleContacts"));
    setWhatsappWa(pathOf("whatsappWa"));
    setWhatsappMedia(pathOf("whatsappMedia"));
    setWhatsappDb(pathOf("whatsappDb"));
    return loaded.backupPath;
  }

  function handleSourceChange(next: string): void {
    // `next` is a source id from the source list, or a method from Platform.
    const picked = importSourceById(next);
    const resolved = picked ? (lastMethodRef.current[picked.id] ?? picked.defaultMethod) : next;
    const gen = ++sourceChangeGenRef.current;
    setSource(resolved);
    lastMethodRef.current[importSourceFor(resolved).id] = resolved;
    setPathStats(emptyImessagePathStats());
    setWhatsappStats(emptyWhatsappPathStats());
    if (resolved !== "whatsapp-ios") setIsBusinessApp(false);
    const loadedBackup = applyRememberedPaths(resolved);

    if (resolved !== "imessage-macos" || loadedBackup.trim() !== "" || !isTauri()) {
      return;
    }

    void (async () => {
      try {
        const home = await invokeHomeDir();
        if (gen !== sourceChangeGenRef.current) return;
        if (home.os !== "macos") return;
        const chatDb = macMessagesDbPath(home.path);
        if (chatDb === "") return;
        const stat = await invokePathStat(chatDb);
        if (gen !== sourceChangeGenRef.current) return;
        const prefill = shouldPrefillMacMessagesDb({
          os: home.os,
          homeDir: home.path,
          chatDb: stat,
          rememberedPath: loadedBackup,
        });
        if (prefill === "") return;
        setBackupPath(prefill);
        if (getRememberImporterPaths()) setImporterPath(resolved, prefill);
      } catch {
        // Home directory and path checks are best-effort on Mac only.
      }
    })();
  }

  const updateBackupPath = (path: string) => {
    setBackupPath(path);
    if (getRememberImporterPaths()) setImporterPath(source, path);
  };

  /** Remember a path field beside the backup, when the source keeps it. */
  const rememberExtraPath = (field: ImporterExtraField, path: string) => {
    if (getRememberImporterPaths() && importSourceFor(source).rememberedPaths.includes(field)) {
      setImporterExtraPath(source, field, path);
    }
  };

  const updateAttachmentRoot = (path: string) => {
    setAttachmentRoot(path);
    rememberExtraPath("attachmentRoot", path);
  };

  const updateAppleContacts = (path: string) => {
    setAppleContacts(path);
    rememberExtraPath("appleContacts", path);
  };

  const updateWhatsappWa = (path: string) => {
    setWhatsappWa(path);
    rememberExtraPath("whatsappWa", path);
  };

  const updateWhatsappMedia = (path: string) => {
    setWhatsappMedia(path);
    rememberExtraPath("whatsappMedia", path);
  };

  const updateWhatsappDb = (path: string) => {
    setWhatsappDb(path);
    rememberExtraPath("whatsappDb", path);
  };

  return (
    <div className={`min-w-0 p-6 ${phase === "form" ? "max-w-[640px]" : "max-w-5xl"}`}>
      {runDirDeleteFailure && (
        <RunDirDeleteFailureNotice
          failure={runDirDeleteFailure}
          onDismiss={dismissRunDirDeleteFailure}
        />
      )}
      {phase === "form" && resumeChecked && resume.kind === "none" && (
        <ImportFormFields
          source={source}
          onSourceChange={handleSourceChange}
          backupPath={backupPath}
          onBackupPathChange={updateBackupPath}
          backupPassword={backupPassword}
          onBackupPasswordChange={setBackupPassword}
          showBackupPassword={showBackupPassword}
          onToggleBackupPassword={() => setShowBackupPassword((v) => !v)}
          attachmentRoot={attachmentRoot}
          onAttachmentRootChange={updateAttachmentRoot}
          appleContacts={appleContacts}
          onAppleContactsChange={updateAppleContacts}
          pathStats={pathStats}
          whatsappKey={whatsappKey}
          onWhatsappKeyChange={setWhatsappKey}
          showWhatsappKey={showWhatsappKey}
          onToggleWhatsappKey={() => setShowWhatsappKey((v) => !v)}
          whatsappWa={whatsappWa}
          onWhatsappWaChange={updateWhatsappWa}
          whatsappMedia={whatsappMedia}
          onWhatsappMediaChange={updateWhatsappMedia}
          whatsappDb={whatsappDb}
          onWhatsappDbChange={updateWhatsappDb}
          isBusinessApp={isBusinessApp}
          onIsBusinessAppChange={setIsBusinessApp}
          whatsappOwnerPhone={whatsappOwnerPhone}
          onWhatsappOwnerPhoneChange={(value) => {
            whatsappOwnerPhoneSeededRef.current = true;
            setWhatsappOwnerPhone(value);
          }}
          whatsappStats={whatsappStats}
          attachments={attachments}
          onAttachmentsChange={setAttachments}
          ownerPhones={ownerPhones}
          onOwnerPhonesChange={(phones) => {
            ownerPhonesSeededRef.current = true;
            setOwnerPhones(phones);
          }}
          ownerEmails={ownerEmails}
          onOwnerEmailsChange={(value) => {
            ownerEmailsSeededRef.current = true;
            setOwnerEmails(value);
          }}
          profilePhones={profilePhones}
          profilePhonesReady={profilePhonesReady}
          profilePhonesError={profilePhonesError}
          showMissingAccountPhoneWarning={
            profilePhonesReady && !profilePhonesError && profilePhones.length === 0
          }
          formatOpen={formatOpen}
          onToggleFormat={() => setFormatOpen((o) => !o)}
          processingOpen={processingOpen}
          onToggleProcessing={() => setProcessingOpen((o) => !o)}
          obfuscate={obfuscate}
          onObfuscateChange={setObfuscate}
          timeZone={timeZone}
          onTimeZoneChange={setTimeZoneOverride}
          phoneCountry={phoneCountry}
          phoneCountries={phoneCountries}
          onPhoneCountryChange={setPhoneCountry}
          running={running}
          onImport={(flushedPhones) =>
            void startImport({
              source,
              backupPath,
              backupPassword,
              ...attachments,
              ownerPhones: flushedPhones ?? ownerPhones,
              ownerEmails: splitEmails(ownerEmails),
              obfuscate,
              timeZone,
              phoneCountry,
              attachmentRoot,
              appleContacts,
              whatsappKey,
              whatsappWa,
              whatsappMedia,
              whatsappDb,
              isBusinessApp,
              whatsappOwnerPhone,
            })
          }
        />
      )}

      {phase === "form" && resumeChecked && resume.kind !== "none" && (
        <ResumeImportPanel
          decision={resume}
          secret={resumeSecret}
          error={resumeError}
          onResume={(typedSecret) => void handleResumeAction(typedSecret)}
          onDiscard={() => void handleDiscardResume()}
        />
      )}

      {(phase === "running" || phase === "done" || reviewWaiting) && (
        <ImportRunView
          phase={phase}
          steps={steps}
          running={running}
          form={form}
          stagingSummary={stagingSummary}
          mediaSummary={mediaSummary}
          mediaFailedCount={mediaFailedCount}
          summaryView={summaryView}
          runDir={runDir}
          importRunId={importRunId}
          completionText={completionText}
          reviewWaiting={reviewWaiting}
          unknownContacts={unknownContacts}
          unknownContactsError={unknownContactsError}
          mediaToolsMissing={mediaToolsMissing}
          mediaPartiallyRan={mediaPartiallyRan}
          identityPanel={
            reviewWaiting === "staging" && sourceIdentities != null ? (
              <BackupIdentityList
                identities={sourceIdentities}
                profile={identityProfile}
                onAdd={addIdentityToProfile}
                busy={running || identityAddBusy}
                error={identityAddError}
                messageCounts={stagingSummary?.ownerIdentities}
              />
            ) : undefined
          }
          reviewBusy={running}
          reviewError={reviewError}
          onApprove={() => void approve()}
          onCancelRun={() => void cancelRun()}
          onCancel={() => void cancel()}
          onBack={returnToForm}
          cancelDisabled={computingSummary}
        />
      )}

      {phase === "identity_stop" && sourceIdentities && (
        <BackupIdentityStopScreen
          identities={sourceIdentities}
          profile={identityProfile}
          onAdd={addIdentityToProfile}
          onContinue={() => void continueAfterIdentityStop()}
          onCancel={cancelIdentityStop}
          busy={running || identityAddBusy}
          error={identityAddError}
        />
      )}
    </div>
  );
}
