import type { ReactNode, SVGProps } from "react";

type IconProps = {
  size?: number;
  className?: string;
} & Omit<SVGProps<SVGSVGElement>, "width" | "height" | "children">;

/**
 * The svg every icon is drawn in. With no `className` the icon gets
 * `shrink-0`; pass `className=""` for an svg with no class at all.
 */
function IconShell({
  size = 13,
  className,
  children,
  ...rest
}: IconProps & { children: ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={className ?? "shrink-0"}
      {...rest}
    >
      {children}
    </svg>
  );
}

/** Edit pencil — diagonal outline with tip and ferrule line. */
export function PencilIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" />
      <path d="m15 5 4 4" />
    </IconShell>
  );
}

/** Delete / trash — lid with handle, can body, two inner lines. */
export function TrashIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M3 6h18" />
      <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
      <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
      <path d="M10 11v6" />
      <path d="M14 11v6" />
    </IconShell>
  );
}

/** Plus — horizontal and vertical stroke. */
export function PlusIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </IconShell>
  );
}

/** Chevron pointing right; rotate 90° when a section is open. */
export function ChevronRightIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="m9 6 6 6-6 6" />
    </IconShell>
  );
}

/** Chevron pointing down — a closed menu that opens below. */
export function ChevronDownIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="m6 9 6 6 6-6" />
    </IconShell>
  );
}

/** Three dots for a row menu. */
export function EllipsisIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <circle cx="5" cy="12" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="19" cy="12" r="1.4" fill="currentColor" stroke="none" />
    </IconShell>
  );
}

/** Two people — a contact group. */
export function PeopleGroupIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <circle cx="9" cy="8" r="3" />
      <path d="M3 19c0-2.8 2.7-5 6-5s6 2.2 6 5" />
      <circle cx="17" cy="9" r="2.4" />
      <path d="M16 14.2c2.4.4 4 2.2 4 4.8" />
    </IconShell>
  );
}

/** Price-tag shape — a message tag. */
export function TagIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M20.6 13.4 12 22l-8.6-8.6a2 2 0 0 1 0-2.8L11.2 3H21v9.8a2 2 0 0 1-.4 1.6Z" />
      <circle cx="16.5" cy="7.5" r="1.2" />
    </IconShell>
  );
}

/** Gear — settings. */
export function GearIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z" />
    </IconShell>
  );
}

/** Open door with an arrow out — log out. */
export function LogOutIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
      <path d="m16 17 5-5-5-5" />
      <path d="M21 12H9" />
    </IconShell>
  );
}

/** Magnifying glass — a saved search. */
export function SearchIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <circle cx="11" cy="11" r="7" />
      <path d="m21 21-4.3-4.3" />
    </IconShell>
  );
}

/** One person — contacts with no group. */
export function PersonIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <circle cx="12" cy="8" r="3" />
      <path d="M5 20c0-3.3 3.1-6 7-6s7 2.7 7 6" />
    </IconShell>
  );
}

/** Check mark — a tool was found. */
export function CheckIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M20 6 9 17l-5-5" />
    </IconShell>
  );
}

/** X mark — a tool was not found. */
export function XIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M18 6 6 18" />
      <path d="m6 6 12 12" />
    </IconShell>
  );
}

/** Padlock — body and shackle. Marks a password field. */
export function LockIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <rect x="4" y="10.5" width="16" height="10" rx="2" />
      <path d="M8 10.5V7a4 4 0 0 1 8 0v3.5" />
    </IconShell>
  );
}

/** Handset — marks an account reached by phone number rather than by address. */
export function PhoneIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      {/* The handset is drawn from y=3.6 to y=16, so its own middle is 2.2
          above the middle of the 24-unit box. Centring the box therefore left
          the glyph sitting high in whatever it was placed in; the shift puts
          the glyph itself on the centre line, which is what the eye reads. */}
      <g transform="translate(0 2.2)">
        <path d="M6.6 3.6h2.6l1.3 3.2-1.9 1.1a10.4 10.4 0 0 0 4.6 4.6l1.1-1.9 3.2 1.3v2.6a1.5 1.5 0 0 1-1.6 1.5A13.6 13.6 0 0 1 5.1 5.2a1.5 1.5 0 0 1 1.5-1.6Z" />
      </g>
    </IconShell>
  );
}

/** Download — an arrow down onto a tray. */
export function DownloadIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M12 3v12" />
      <path d="m7 10 5 5 5-5" />
      <path d="M5 21h14" />
    </IconShell>
  );
}

/** Play — a triangle pointing right, filled. */
export function PlayIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M7 4.5v15l12-7.5Z" fill="currentColor" />
    </IconShell>
  );
}

/**
 * Small chevron pointing down — the closed state of a select or combo box.
 * Drawn on a 10-unit grid with a 1.5 stroke, heavier than `ChevronDownIcon`, so
 * it still reads at 10 pixels.
 */
export function SelectChevronIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} viewBox="0 0 10 10" strokeWidth="1.5" {...rest}>
      <path d="M2.5 3.5 5 6l2.5-2.5" />
    </IconShell>
  );
}

/**
 * Magnifying glass inside the search box. Its handle is shorter than
 * `SearchIcon`'s (it ends at 20,20, not 21,21), which keeps the glyph compact
 * at the left edge of the box.
 */
export function SearchFieldIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <circle cx="11" cy="11" r="7" />
      <path d="M20 20l-3.5-3.5" />
    </IconShell>
  );
}

/** Clock face — a recent search. */
export function ClockIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </IconShell>
  );
}

/** Three sliders — advanced search. */
export function SlidersIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3" />
      <path d="M1 14h6M9 8h6M17 16h6" />
    </IconShell>
  );
}

/** Calendar page — opens a date picker. */
export function CalendarIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <rect x="3" y="4" width="18" height="18" rx="2" />
      <path d="M16 2v4" />
      <path d="M8 2v4" />
      <path d="M3 10h18" />
    </IconShell>
  );
}

/** An arrow up beside an arrow down — the sort menu. */
export function SortIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} viewBox="0 0 16 16" strokeWidth="1.5" {...rest}>
      <path d="M5 3v10M5 3l-2.5 2.5M5 3l2.5 2.5M11 13V3M11 13l-2.5-2.5M11 13l2.5-2.5" />
    </IconShell>
  );
}

/**
 * Check mark beside the chosen item of a menu. Drawn on a 12-unit grid with a
 * 1.8 stroke, so at 14 pixels it is heavier and wider-set than `CheckIcon`.
 */
export function MenuCheckIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} viewBox="0 0 12 12" strokeWidth="1.8" {...rest}>
      <path d="M2 6.2L4.6 9 10 3" />
    </IconShell>
  );
}

/**
 * Check mark inside a chosen radio dot or a ticked checkbox. Drawn on a
 * 16-unit grid with a 2.25 stroke, heavier than `CheckIcon`, so it still reads
 * at 12 pixels inside a small control.
 */
export function ToggleCheckIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} viewBox="0 0 16 16" strokeWidth="2.25" {...rest}>
      <path d="M3.5 8.5 6.5 11.5 12.5 4.5" />
    </IconShell>
  );
}

/** Two sheets, one over the other — copy. */
export function CopyIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </IconShell>
  );
}

/** Two people, one behind the other — a group conversation. */
export function GroupConversationIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
      <circle cx="7" cy="7" r="4" />
      <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
      <path d="M16 3.13a4 4 0 0 1 0 7.75" />
    </IconShell>
  );
}

/** Message bubble — Conversations. */
export function ConversationsIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
    </IconShell>
  );
}

/** Address book with a person on the cover — Contacts. */
export function ContactsIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
      <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
      <circle cx="12" cy="8" r="2" />
      <path d="M9 14c0-1.1 1.3-2 3-2s3 .9 3 2" />
    </IconShell>
  );
}

/** An arrow down into a tray — Import. */
export function ImportIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M12 3v12" />
      <path d="m8 11 4 4 4-4" />
      <path d="M4 19h16" />
    </IconShell>
  );
}

/** An arrow up out of a tray — Export. */
export function ExportIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M12 15V3" />
      <path d="m8 7 4-4 4 4" />
      <path d="M4 19h16" />
    </IconShell>
  );
}

/** Open eye — the password is hidden, and pressing shows it. */
export function EyeIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
      <circle cx="12" cy="12" r="3" />
    </IconShell>
  );
}

/** Eye struck through — the password is shown, and pressing hides it. */
export function EyeOffIcon({ size, className, ...rest }: IconProps) {
  return (
    <IconShell size={size} className={className} {...rest}>
      <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94" />
      <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19" />
      <path d="M14.12 14.12a3 3 0 1 1-4.24-4.24" />
      <line x1="1" y1="1" x2="23" y2="23" />
    </IconShell>
  );
}
