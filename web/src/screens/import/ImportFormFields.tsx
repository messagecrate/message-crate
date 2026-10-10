import { useEffect, useRef, useState } from "react";
import Button from "../../components/Button";
import Checkbox from "../../components/Checkbox";
import PathPicker from "../../components/PathPicker";
import type { PhoneTokenFieldHandle } from "../../components/PhoneTokenField";
import { phoneCountryItems } from "../../components/phoneCountryItems";
import Select, { ListBoxItem, selectItemClassName } from "../../components/Select";
import TimeZoneField from "../../components/TimeZoneField";
import { desktopJobRunningText, useDesktopJob } from "../../lib/desktopJob";
import type { ImessagePathStats } from "../../lib/imessageImport";
import { IMPORT_SOURCES, importSourceById, importSourceFor } from "../../lib/importSources";
import { FieldStatus } from "../../lib/importSources/FieldStatus";
import type { OwnerPhoneEntry } from "../../lib/importSources/types";
import { WhatsappFallbackPhoneField } from "../../lib/importSources/WhatsappFormSection";
import type { PhoneCountryChoice } from "../../lib/phoneCountries";
import { ownerPhonesNeedMismatchAck } from "../../lib/phoneTokens";
import { parseSelectKey } from "../../lib/selectKey";
import { toolUsable } from "../../lib/tauri";
import type { AttachmentChoices, AttachmentMediaMode } from "../../lib/types";
import { useToolsStatus } from "../../lib/useToolsStatus";
import type { WhatsappPathStats } from "../../lib/whatsappImport";
import {
  ATTACHMENT_OPTIONS,
  CollapsibleSection,
  fieldStyle,
  hintStyle,
  RESOLUTION_OPTIONS,
  StackedField,
  sectionGap,
} from "./ImportFormUi";
import { MissingProgramNotice } from "./MissingProgramNotice";
import { mediaJobVerb } from "./reviewForecast";

export type ImportFormFieldsProps = {
  source: string;
  onSourceChange: (source: string) => void;
  backupPath: string;
  onBackupPathChange: (path: string) => void;
  backupPassword: string;
  onBackupPasswordChange: (value: string) => void;
  showBackupPassword: boolean;
  onToggleBackupPassword: () => void;
  attachmentRoot: string;
  onAttachmentRootChange: (path: string) => void;
  appleContacts: string;
  onAppleContactsChange: (path: string) => void;
  pathStats: ImessagePathStats;
  whatsappKey: string;
  onWhatsappKeyChange: (value: string) => void;
  showWhatsappKey: boolean;
  onToggleWhatsappKey: () => void;
  whatsappWa: string;
  onWhatsappWaChange: (path: string) => void;
  whatsappMedia: string;
  onWhatsappMediaChange: (path: string) => void;
  whatsappDb: string;
  onWhatsappDbChange: (path: string) => void;
  whatsappBusiness: boolean;
  onWhatsappBusinessChange: (value: boolean) => void;
  /** The holder's WhatsApp number: required on Android, a fallback on iPhone. */
  whatsappOwnerPhone: string;
  onWhatsappOwnerPhoneChange: (value: string) => void;
  whatsappStats: WhatsappPathStats;
  attachments: AttachmentChoices;
  onAttachmentsChange: (attachments: AttachmentChoices) => void;
  ownerPhones: string[];
  onOwnerPhonesChange: (phones: string[]) => void;
  /** Owner email addresses as typed (SMS Backup+ only); commas separate several. */
  ownerEmails: string;
  onOwnerEmailsChange: (value: string) => void;
  /** The account's phones for SBR mismatch checks (empty until loaded). */
  profilePhones: string[];
  profilePhonesReady: boolean;
  /** True when the profile request failed (fail open on the mismatch check). */
  profilePhonesError: boolean;
  showMissingAccountPhoneWarning: boolean;
  formatOpen: boolean;
  onToggleFormat: () => void;
  processingOpen: boolean;
  onToggleProcessing: () => void;
  obfuscate: boolean;
  onObfuscateChange: (value: boolean) => void;
  /** The IANA zone iMazing dates are read in; shown only for that source. */
  timeZone: string;
  onTimeZoneChange: (zone: string) => void;
  /** The phone's country as an ISO code, or empty for none; every source has it. */
  phoneCountry: string;
  /** The countries to offer, from `GET /v1/phone-countries`; empty until loaded. */
  phoneCountries: readonly PhoneCountryChoice[];
  onPhoneCountryChange: (code: string) => void;
  running: boolean;
  /** Optional flushed owner phones (SBR commits draft before import). */
  onImport: (ownerPhones?: string[]) => void;
};

/** The choice that states no country for the phone. */
const NO_PHONE_COUNTRY = "none";

/**
 * The country of the phone the backup came from. A backup writes many numbers
 * without their `+` code, as the phone showed them, and nothing in it says
 * which country they are in. With a country picked, each is read as a number
 * there, so `07700 900123` and `+44 7700 900123` are one person. With none,
 * such a number keeps its digits and can be given its country later on the
 * Contacts screen (#1676).
 */
function PhoneCountryField({
  value,
  countries,
  onChange,
}: {
  value: string;
  countries: readonly PhoneCountryChoice[];
  onChange: (code: string) => void;
}) {
  return (
    <StackedField label="Phone's country" optional>
      <Select
        selectedKey={value || NO_PHONE_COUNTRY}
        onSelectionChange={(k) => {
          if (typeof k === "string") onChange(k === NO_PHONE_COUNTRY ? "" : k);
        }}
        aria-label="Phone's country"
        triggerClassName="!bg-bg"
      >
        {[
          <ListBoxItem
            key={NO_PHONE_COUNTRY}
            id={NO_PHONE_COUNTRY}
            textValue="Not stated"
            className={selectItemClassName}
          >
            Not stated
          </ListBoxItem>,
          ...phoneCountryItems(countries),
        ]}
      </Select>
      <p className={hintStyle}>
        Numbers written without a country code are read as numbers in this country. Leave it unset
        when the backup holds numbers from more than one country written that way.
      </p>
    </StackedField>
  );
}

const attachmentHelp: Record<AttachmentMediaMode, string> = {
  copy: "Copy all files as is",
  convert: "Convert all files to common formats (.jpg, .mp4, .mp3) at high quality",
  compress: "Re-encodes for smaller file size at the expense of some quality",
  skip: "Do not copy files",
};

function AttachmentFields(props: {
  attachments: AttachmentChoices;
  onAttachmentsChange: (attachments: AttachmentChoices) => void;
  showCompress: boolean;
}) {
  const { attachments, onAttachmentsChange } = props;
  return (
    <>
      <StackedField label="Attachments">
        <Select
          selectedKey={attachments.attachmentMedia}
          onSelectionChange={(k) => {
            const mode = parseSelectKey(k, ["copy", "convert", "compress", "skip"] as const);
            if (mode) onAttachmentsChange({ ...attachments, attachmentMedia: mode });
          }}
          aria-label="Attachments"
          triggerClassName="!bg-bg"
        >
          {ATTACHMENT_OPTIONS.map((o) => (
            <ListBoxItem key={o.id} id={o.id} className={selectItemClassName}>
              {o.label}
            </ListBoxItem>
          ))}
        </Select>
        <p className={hintStyle}>{attachmentHelp[attachments.attachmentMedia]}</p>
      </StackedField>

      {props.showCompress && (
        <div className="mb-[1.1rem] ml-4">
          <StackedField label="Target resolution">
            <Select
              selectedKey={attachments.maxResolution}
              onSelectionChange={(k) =>
                onAttachmentsChange({ ...attachments, maxResolution: String(k) })
              }
              aria-label="Target resolution"
              triggerClassName="!bg-bg"
            >
              {RESOLUTION_OPTIONS.map((r) => (
                <ListBoxItem key={r} id={r} className={selectItemClassName}>
                  {r.replace("p", "")}
                </ListBoxItem>
              ))}
            </Select>
            <p className={hintStyle}>Maximum video resolution; videos are not upscaled.</p>
          </StackedField>
          {/* The desktop app's refusals name Max FPS and Minimum Video File Size by
              these labels (media::compress_options_from_form in crates/libs/media), so
              a renamed label is renamed there too. */}
          <StackedField label="Max FPS">
            <input
              type="text"
              value={attachments.maxFps}
              onChange={(e) => onAttachmentsChange({ ...attachments, maxFps: e.target.value })}
              className={fieldStyle}
            />
            <p className={hintStyle}>
              Maximum video frame rate; a video with a lower frame rate keeps it.
            </p>
          </StackedField>
          <StackedField label="Minimum Video File Size (Megabytes)">
            <input
              type="text"
              value={attachments.minSizeMb}
              onChange={(e) => onAttachmentsChange({ ...attachments, minSizeMb: e.target.value })}
              className={fieldStyle}
            />
            <p className={hintStyle}>Only re-encode videos above this size.</p>
          </StackedField>
        </div>
      )}
    </>
  );
}

export default function ImportFormFields(props: ImportFormFieldsProps) {
  // Every per-source rule below is read from the selected source's descriptor.
  const source = importSourceFor(props.source);
  const asksOwnerPhones = source.asksOwnerPhones;
  const backup = source.backupField(props.source);
  const processingOptions = source.processingOptions(props.source);

  const phoneFieldRef = useRef<PhoneTokenFieldHandle>(null);
  const [phoneDraft, setPhoneDraft] = useState("");
  const [mismatchAck, setMismatchAck] = useState(false);
  const phoneDraftPending = phoneDraft.trim().length > 0;
  const phonesForMatch = phoneDraftPending
    ? [...props.ownerPhones, phoneDraft.trim()]
    : props.ownerPhones;
  const phonesMismatch =
    asksOwnerPhones &&
    ownerPhonesNeedMismatchAck(phonesForMatch, props.profilePhones, {
      ready: props.profilePhonesReady,
      fetchFailed: props.profilePhonesError,
    });

  useEffect(() => {
    if (!phonesMismatch) setMismatchAck(false);
  }, [phonesMismatch]);

  useEffect(() => {
    if (!asksOwnerPhones) {
      setPhoneDraft("");
      setMismatchAck(false);
    }
  }, [asksOwnerPhones]);

  const ownerPhoneEntry: OwnerPhoneEntry = {
    fieldRef: phoneFieldRef,
    onDraftChange: setPhoneDraft,
    draftPending: phoneDraftPending,
    phonesForMatch,
    mismatch: phonesMismatch,
    mismatchAck,
    onMismatchAckChange: setMismatchAck,
  };
  // The asterisks and the Import button both read the source's readiness,
  // so a field cannot be needed and unmarked.
  const readiness = source.readiness({ ...props, ownerPhoneEntry }, props.source);

  const showCompress =
    source.showsAttachmentOptions && props.attachments.attachmentMedia === "compress";

  // The desktop runs one job at a time, so an Export or a Convert that
  // is running would make it refuse the Import Run's first job.
  const runningJob = useDesktopJob();
  const blockedBy = runningJob !== null && runningJob !== "Import Run" ? runningJob : null;

  // A WhatsApp import runs wtsexporter and can't start without it; one still
  // downloading is waited for. Convert and Compress run ffmpeg and ffprobe
  // after Staging, so a missing one is said here and blocks nothing yet.
  const tools = useToolsStatus().data ?? null;
  const wtsexporterBlocked =
    source.needsWtsexporter && tools !== null && !toolUsable(tools.wtsexporter);
  const mediaRunsFfmpeg =
    source.showsAttachmentOptions && mediaJobVerb(props.attachments.attachmentMedia) !== null;

  const canImport =
    blockedBy === null && !wtsexporterBlocked && readiness.enabled && !props.running;

  function handleImport(): void {
    if (!asksOwnerPhones) {
      props.onImport();
      return;
    }
    // Commit the number being typed, then ask the source again with the
    // committed list, which is what the run gets.
    const phones = phoneFieldRef.current?.flush() ?? props.ownerPhones;
    const committed: OwnerPhoneEntry = {
      ...ownerPhoneEntry,
      draftPending: false,
      phonesForMatch: phones,
      mismatch: ownerPhonesNeedMismatchAck(phones, props.profilePhones, {
        ready: props.profilePhonesReady,
        fetchFailed: props.profilePhonesError,
      }),
    };
    const ready = source.readiness(
      { ...props, ownerPhones: phones, ownerPhoneEntry: committed },
      props.source,
    );
    if (ready.enabled) props.onImport(phones);
  }

  const attachmentFields = (
    <AttachmentFields
      attachments={props.attachments}
      onAttachmentsChange={props.onAttachmentsChange}
      showCompress={showCompress}
    />
  );

  return (
    <>
      <h1 className="m-0 mb-1 text-2xl font-bold">Import Messages</h1>
      <p className="m-0 mb-5 text-[0.875rem] text-muted">Select your messages.</p>

      <CollapsibleSection
        title="Import Messages"
        open={props.formatOpen}
        onToggle={props.onToggleFormat}
      >
        <div className={sectionGap}>
          <Select
            selectedKey={source.id}
            onSelectionChange={(k) => {
              const key = String(k);
              if (importSourceById(key)) props.onSourceChange(key);
            }}
            aria-label="Import source"
            triggerClassName="!bg-bg"
          >
            {IMPORT_SOURCES.map((s) => (
              <ListBoxItem key={s.id} id={s.id} className={selectItemClassName}>
                {s.label}
              </ListBoxItem>
            ))}
          </Select>
        </div>

        {source.methods.length > 1 ? (
          <StackedField label="Platform">
            <Select
              selectedKey={props.source}
              onSelectionChange={(k) => {
                const key = String(k);
                if (source.methods.some((m) => m.id === key)) props.onSourceChange(key);
              }}
              aria-label="Platform"
              triggerClassName="!bg-bg"
            >
              {source.visibleMethods(props.source).map((m) => (
                <ListBoxItem key={m.id} id={m.id} className={selectItemClassName}>
                  {m.label}
                </ListBoxItem>
              ))}
            </Select>
          </StackedField>
        ) : null}

        <StackedField label={backup.label} required>
          <PathPicker
            value={props.backupPath}
            onChange={props.onBackupPathChange}
            directory={backup.directory}
            filters={backup.filters}
            placeholder={backup.placeholder}
          />
          {backup.hint ? <p className={hintStyle}>{backup.hint}</p> : null}
          <FieldStatus message={readiness.errors.backupPath} />
        </StackedField>

        <source.FormSection
          {...props}
          descriptor={source}
          ownerPhoneEntry={ownerPhoneEntry}
          errors={readiness.errors}
          attachmentFields={attachmentFields}
        />
      </CollapsibleSection>

      <CollapsibleSection
        title="Processing Options (Advanced)"
        open={props.processingOpen}
        onToggle={props.onToggleProcessing}
      >
        <div className="mb-2 flex flex-col items-start gap-3">
          {processingOptions.includes("obfuscate") ? (
            <Checkbox
              labelClassName="text-[0.875rem]"
              checked={props.obfuscate}
              onChange={props.onObfuscateChange}
            >
              Obfuscate - All message data is anonymized.
            </Checkbox>
          ) : null}
          {processingOptions.includes("timeZone") ? (
            <div className="w-full max-w-[28rem]">
              <TimeZoneField
                label="Time zone of the messages"
                value={props.timeZone}
                onChange={props.onTimeZoneChange}
              />
              <p className={hintStyle}>
                Pre-filled from your profile. iMazing writes each message time without a zone, so
                pick the one the phone was in.
              </p>
            </div>
          ) : null}
        </div>
        <PhoneCountryField
          value={props.phoneCountry}
          countries={props.phoneCountries}
          onChange={props.onPhoneCountryChange}
        />
        {processingOptions.includes("whatsappFallbackPhone") ? (
          <WhatsappFallbackPhoneField
            value={props.whatsappOwnerPhone}
            onChange={props.onWhatsappOwnerPhoneChange}
          />
        ) : null}
      </CollapsibleSection>

      {blockedBy ? (
        <p role="status" className="mt-2 mb-0 text-[0.813rem] text-muted">
          {desktopJobRunningText(blockedBy, "Import")}
        </p>
      ) : null}

      {tools && source.needsWtsexporter ? (
        <MissingProgramNotice programs={["wtsexporter"]} status={tools} need="whatsapp" />
      ) : null}
      {tools && mediaRunsFfmpeg ? (
        <MissingProgramNotice programs={["ffmpeg", "ffprobe"]} status={tools} need="media" />
      ) : null}

      <div className="mt-2 flex gap-3">
        <Button
          variant="primary"
          onClick={handleImport}
          disabled={!canImport}
          className="!rounded-lg !px-6 !py-[0.55rem]"
        >
          Import
        </Button>
      </div>
    </>
  );
}
