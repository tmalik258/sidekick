// Interface icons from Solar by 480 Design (CC BY 4.0,
// https://creativecommons.org/licenses/by/4.0/), via Iconify (@iconify-json/solar).
// Inlined so the app works offline. Bodies are static, trusted SVG markup.

const BODIES = {
  folder:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"><path d="M4 11.5V5.71231C4 5.05041 4 4.71946 4.05548 4.44379C4.29971 3.23023 5.31225 2.28098 6.60671 2.05201C6.90076 2 7.25377 2 7.9598 2C8.26914 2 8.42381 2 8.57246 2.01303C9.21332 2.06921 9.82122 2.30528 10.3168 2.69039C10.4317 2.77971 10.5411 2.88224 10.7598 3.08731L11.2 3.5C11.8526 4.11183 12.1789 4.41775 12.5697 4.62157C12.7844 4.73353 13.012 4.82195 13.2483 4.88508C13.6783 5 14.1398 5 15.0627 5H15.3617C17.4676 5 18.5205 5 19.2049 5.5771C19.2679 5.63018 19.3278 5.68635 19.3844 5.74537C20 6.38701 20 7.37415 20 9.34843V11.5"/><path d="M10 17H14"/><path d="M3.47674 17.4839C2.99958 14.7678 2.761 13.4097 3.33908 12.433C3.4866 12.1838 3.66852 11.9582 3.87908 11.7634C4.7042 11 6.0379 11 8.7053 11H15.2947C17.9621 11 19.2958 11 20.1209 11.7634C20.3315 11.9582 20.5134 12.1838 20.6609 12.433C21.239 13.4097 21.0004 14.7678 20.5233 17.4839C20.1798 19.4391 20.008 20.4167 19.4129 21.0655C19.2585 21.2338 19.0858 21.383 18.8982 21.5101C18.175 22 17.2149 22 15.2947 22H8.70531C6.7851 22 5.825 22 5.10183 21.5101C4.9142 21.383 4.74145 21.2338 4.58706 21.0655C3.99198 20.4167 3.82024 19.4391 3.47674 17.4839Z"/></g>',
  settings:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"><path d="M7.84308 3.80211C9.8718 2.6007 10.8862 2 12 2C13.1138 2 14.1282 2.6007 16.1569 3.80211L16.8431 4.20846C18.8718 5.40987 19.8862 6.01057 20.4431 7C21 7.98943 21 9.19084 21 11.5937V12.4063C21 14.8092 21 16.0106 20.4431 17C19.8862 17.9894 18.8718 18.5901 16.8431 19.7915L16.1569 20.1979C14.1282 21.3993 13.1138 22 12 22C10.8862 22 9.8718 21.3993 7.84308 20.1979L7.15692 19.7915C5.1282 18.5901 4.11384 17.9894 3.55692 17C3 16.0106 3 14.8092 3 12.4063V11.5937C3 9.19084 3 7.98943 3.55692 7C4.11384 6.01057 5.1282 5.40987 7.15692 4.20846L7.84308 3.80211Z"/><circle cx="12" cy="12" r="3"/></g>',
  pause:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"><path d="M2 6C2 4.11438 2 3.17157 2.58579 2.58579C3.17157 2 4.11438 2 6 2C7.88562 2 8.82843 2 9.41421 2.58579C10 3.17157 10 4.11438 10 6V18C10 19.8856 10 20.8284 9.41421 21.4142C8.82843 22 7.88562 22 6 22C4.11438 22 3.17157 22 2.58579 21.4142C2 20.8284 2 19.8856 2 18V6Z"/><path d="M14 6C14 4.11438 14 3.17157 14.5858 2.58579C15.1716 2 16.1144 2 18 2C19.8856 2 20.8284 2 21.4142 2.58579C22 3.17157 22 4.11438 22 6V18C22 19.8856 22 20.8284 21.4142 21.4142C20.8284 22 19.8856 22 18 22C16.1144 22 15.1716 22 14.5858 21.4142C14 20.8284 14 19.8856 14 18V6Z"/></g>',
  play: '<path fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" d="M20.4086 9.35258C22.5305 10.5065 22.5305 13.4935 20.4086 14.6474L7.59662 21.6145C5.53435 22.736 3 21.2763 3 18.9671L3 5.0329C3 2.72368 5.53435 1.26402 7.59661 2.38548L20.4086 9.35258Z"/>',
  close:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"><circle cx="12" cy="12" r="10"/><path d="M14.5 9.50002L9.5 14.5M9.49998 9.5L14.5 14.5"/></g>',
  // Drawn for Duskie (not from Solar), in the same 1.5 stroke style.
  plus: '<path fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" d="M12 5v14M5 12h14"/>',
  ask: '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></g>',
  focus:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="5"/><circle cx="12" cy="12" r="1.5"/></g>',
  undo: '<path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M4 8h10.5a5.5 5.5 0 0 1 0 11H9M4 8l3.5-3.5M4 8l3.5 3.5"/>',
  screen:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5"><rect x="2.5" y="3.5" width="19" height="13" rx="2.5"/><path d="M8 20.5h8M12 16.5v4"/></g>',
  speaker:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5"><path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11"/></g>',
  speakerOff:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5"><path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="m16 9.5 5 5M21 9.5l-5 5"/></g>',
  chevron:
    '<path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.6" d="m7 10 5 5 5-5"/>',
  check:
    '<path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M5 12.5 10 17 19 7.5"/>',
  mic: '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"><rect x="8.5" y="2.5" width="7" height="12" rx="3.5"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3.5"/></g>',
  // Drawn for Duskie (not from Solar), in the same 1.5 stroke style.
  history:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.5 2"/></g>',
  // Drawn for Duskie (not from Solar), in the same 1.5 stroke style.
  file: '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5"><path d="M14 2.5H7.5a2 2 0 0 0-2 2v15a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2V7L14 2.5Z"/><path d="M14 2.5V7h4.5M9 12.5h6M9 16h4"/></g>',
  wifi: '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"><path d="M2 8.8a15 15 0 0 1 20 0"/><path d="M5.3 12.5a10 10 0 0 1 13.4 0"/><path d="M8.6 16.1a5 5 0 0 1 6.8 0"/></g><circle cx="12" cy="19.5" r="1.25" fill="currentColor"/>',
  update:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5"><circle cx="12" cy="12" r="9"/><path d="M12 16.5v-9M8.5 11 12 7.5l3.5 3.5"/></g>',
  // Drawn for Duskie (not from Solar), in the same 1.5 stroke style.
  refresh:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5"><path d="M19.5 12a7.5 7.5 0 0 1-13.2 4.9M4.5 12A7.5 7.5 0 0 1 17.7 7.1"/><path d="M19.5 7.5V12h-4.5M4.5 16.5V12H9"/></g>',
  wifiOff:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"><path d="M2 8.8a15 15 0 0 1 20 0"/><path d="M5.3 12.5a10 10 0 0 1 13.4 0"/><path d="M8.6 16.1a5 5 0 0 1 6.8 0"/><path d="M3.5 3.5l17 17"/></g><circle cx="12" cy="19.5" r="1.25" fill="currentColor"/>',
  // Drawn for Duskie (not from Solar).
  lock: '<g fill="currentColor"><rect x="4.5" y="10.5" width="15" height="11" rx="2.5"/><path fill="none" stroke="currentColor" stroke-width="2.2" d="M8 10.5V7.5a4 4 0 0 1 8 0v3"/></g>',
  // Drawn for Duskie (not from Solar), in the same 1.5 stroke style.
  sparkle:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5"><path d="M10 3.5 11.8 9l5.5 1.8-5.5 1.8L10 18l-1.8-5.4L2.7 10.8 8.2 9z"/><path d="M18.5 3v4M16.5 5h4M18.5 16v4M16.5 18h4"/></g>',
  terminal:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5"><path d="m5 8 4 4-4 4M12 17h7"/></g>',
} as const;

export type IconName = keyof typeof BODIES;

export function Icon({ name, size = 16, className }: { name: IconName; size?: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      aria-hidden="true"
      className={className}
      // biome-ignore lint/security/noDangerouslySetInnerHtml: static icon bodies defined above
      dangerouslySetInnerHTML={{ __html: BODIES[name] }}
    />
  );
}

/** The design's small 16px line icons, for tabs, nav rows and chips. */
const SMALL = {
  dl: '<path d="M8 2.5v7M5 7l3 3 3-3M3 12.5h10"/>',
  restart: '<path d="M13 8a5 5 0 11-1.6-3.7M13 2.8v3h-3"/>',
  info: '<circle cx="8" cy="8" r="6"/><path d="M8 7.2v3.6"/><circle cx="8" cy="5" r=".4" fill="currentColor"/>',
  diff: '<path d="M5 2.5v5M2.5 5h5M2.5 11.5h5"/><path d="M11 2v12"/><path d="M13.5 9.5 11 12l-2.5-2.5" opacity=".55"/>',
  check: '<path d="M3.5 8.5l3 3 6-7"/>',
  x: '<path d="M4 4l8 8M12 4l-8 8"/>',
  mic: '<rect x="6" y="1.8" width="4" height="8" rx="2"/><path d="M3.5 7.8a4.5 4.5 0 009 0M8 12.3v1.9"/>',
  send: '<path d="M8 13V3M3.5 7.5L8 3l4.5 4.5"/>',
  branch:
    '<circle cx="4.5" cy="3.5" r="1.5"/><circle cx="4.5" cy="12.5" r="1.5"/><circle cx="11.5" cy="5" r="1.5"/><path d="M4.5 5v6M11.5 6.5c0 3.2-7 2-7 4.5"/>',
  up: '<path d="M8 13V3.5M4.5 7L8 3.5 11.5 7"/>',
  down: '<path d="M8 3v9.5M4.5 9L8 12.5 11.5 9"/>',
  refresh: '<path d="M13 8a5 5 0 11-1.6-3.7M13 2.8v3h-3"/>',
  search: '<circle cx="7" cy="7" r="4.5"/><path d="M10.4 10.4L14 14"/>',
  chev: '<path d="M4.5 6.2L8 9.7l3.5-3.5"/>',
  chevr: '<path d="M6.2 4.5L9.7 8l-3.5 3.5"/>',
  commit: '<circle cx="8" cy="8" r="2.6"/><path d="M1.5 8h3.9M10.6 8h3.9"/>',
  pr: '<circle cx="4" cy="3.5" r="1.5"/><circle cx="4" cy="12.5" r="1.5"/><circle cx="12" cy="12.5" r="1.5"/><path d="M4 5v6M12 11V6.5a2 2 0 00-2-2H7.2M8.7 3L7.2 4.5 8.7 6"/>',
  plus: '<path d="M8 3.2v9.6M3.2 8h9.6"/>',
  minus: '<path d="M3.2 8h9.6"/>',
  trash: '<path d="M3 4.5h10M6.4 4.5V3h3.2v1.5M4.4 4.5l.6 8.5h6l.6-8.5"/>',
  undo: '<path d="M5 6.5h5.2a2.9 2.9 0 010 5.8H6.5M5 6.5L7.4 4M5 6.5L7.4 9"/>',
  spark: '<path d="M8 2l1.3 3.7L13 7l-3.7 1.3L8 12l-1.3-3.7L3 7l3.7-1.3z"/>',
  home: '<path d="M2.5 7.3L8 2.8l5.5 4.5V13h-3.7v-3H6.2v3H2.5z"/>',
  palette:
    '<path d="M8 2.2a5.8 5.8 0 100 11.6c.9 0 1.3-.7 1-1.5-.4-.9.2-1.8 1.2-1.8h1.4a2.2 2.2 0 002.2-2.2A6 6 0 008 2.2z"/><circle cx="5.2" cy="7" r=".8"/><circle cx="7.8" cy="4.9" r=".8"/><circle cx="10.6" cy="6.2" r=".8"/>',
  bell: '<path d="M4 11V7a4 4 0 018 0v4l1.2 1.5H2.8zM6.6 13.8a1.6 1.6 0 002.8 0"/>',
  cpu: '<rect x="4" y="4" width="8" height="8" rx="1.5"/><path d="M6.5 1.8V4M9.5 1.8V4M6.5 12v2.2M9.5 12v2.2M1.8 6.5H4M1.8 9.5H4M12 6.5h2.2M12 9.5h2.2"/>',
  brain:
    '<path d="M6 2.5a2.2 2.2 0 00-2.2 2.2 2.3 2.3 0 00-1 4 2.3 2.3 0 002.3 3.6A1.9 1.9 0 008 13V3.6A1.7 1.7 0 006 2.5zM10 2.5a2.2 2.2 0 012.2 2.2 2.3 2.3 0 011 4 2.3 2.3 0 01-2.3 3.6A1.9 1.9 0 018 13"/>',
  bolt: '<path d="M9 1.8L3.5 9H8l-1 5.2L12.5 7H8z"/>',
  plug: '<path d="M5.5 2v3M10.5 2v3M3.8 5h8.4v2.5a4.2 4.2 0 01-8.4 0zM8 11.7V14"/>',
  key: '<rect x="1.8" y="4.5" width="12.4" height="7" rx="1.5"/><path d="M4.5 7h.01M7 7h.01M9.5 7h.01M12 7h.01M5 9.2h6"/>',
  shield: '<path d="M8 1.8l5 2v4c0 3-2.2 5.3-5 6.4-2.8-1.1-5-3.4-5-6.4v-4z"/>',
  disk: '<ellipse cx="8" cy="4" rx="5.5" ry="2"/><path d="M2.5 4v8c0 1.1 2.5 2 5.5 2s5.5-.9 5.5-2V4M2.5 8c0 1.1 2.5 2 5.5 2s5.5-.9 5.5-2"/>',
  file: '<path d="M4 1.8h5.2L12 4.6v9.6H4zM9 1.8v3h3"/>',
  folder: '<path d="M2 4.2h4.3l1.3 1.5H14V13H2z"/>',
  more: '<circle cx="3.5" cy="8" r=".9"/><circle cx="8" cy="8" r=".9"/><circle cx="12.5" cy="8" r=".9"/>',
  open: '<path d="M9 2.5h4.5V7M13.5 2.5L7.5 8.5M11.5 9.5v4h-9v-9h4"/>',
  box: '<path d="M2.5 5l5.5-2.5L13.5 5v6.5L8 14l-5.5-2.5zM2.5 5L8 7.5 13.5 5M8 7.5V14"/>',
  wifi: '<path d="M2 6.2a9 9 0 0112 0M4.2 8.6a5.8 5.8 0 017.6 0M6.4 11a2.6 2.6 0 013.2 0M2.5 2.5l11 11"/>',
  clock: '<circle cx="8" cy="8" r="5.8"/><path d="M8 4.8V8l2.2 1.4"/>',
  copy: '<rect x="5" y="5" width="8.5" height="8.5" rx="1.5"/><path d="M11 5V3.5A1.5 1.5 0 009.5 2h-6A1.5 1.5 0 002 3.5v6A1.5 1.5 0 003.5 11H5"/>',
  dot: '<circle cx="8" cy="8" r="3"/>',
  play: '<path d="M5 3.5l7 4.5-7 4.5z"/>',
  pause: '<path d="M5.5 3.5v9M10.5 3.5v9"/>',
  stop: '<rect x="4" y="4" width="8" height="8" rx="1.5"/>',
  wave: '<path d="M2 8h1.5M5 5v6M8 3v10M11 5.5v5M14 8h-1"/>',
  people:
    '<circle cx="6" cy="5.5" r="2.3"/><path d="M1.8 13.2c.6-2.3 2.2-3.5 4.2-3.5s3.6 1.2 4.2 3.5M10.8 3.4a2.2 2.2 0 010 4.2M12 9.9c1.2.5 2 1.6 2.3 3.3"/>',
  warn: '<path d="M8 2.2l6 10.8H2zM8 6.6v3M8 11.4h.01"/>',
  gear: '<circle cx="8" cy="8" r="2.2"/><path d="M8 1.8v1.6M8 12.6v1.6M1.8 8h1.6M12.6 8h1.6M3.6 3.6l1.15 1.15M11.25 11.25l1.15 1.15M3.6 12.4l1.15-1.15M11.25 4.75l1.15-1.15"/><circle cx="8" cy="8" r="4.6"/>',
} as const;

/** Design names that use the 24px set above instead. */
const BIG: Record<string, IconName> = {
  x: "close",
  plus: "plus",
  pause: "pause",
  play: "play",
  check: "check",
  folder: "folder",
  file: "file",
  clock: "history",
  history: "history",
  mic: "mic",
  gear: "settings",
  search: "ask",
  focus: "focus",
  ask: "ask",
  chev: "chevron",
  chevr: "chevron",
  undo: "undo",
  term: "terminal",
  wifi: "wifi",
  update: "update",
  lock: "lock",
};

export type IcName = keyof typeof SMALL | keyof typeof BIG;

/** An icon by its design name: the 24px body when it has one, else the small set. */
export function Ic({ name, size = 16, weight = 1.5 }: { name: IcName; size?: number; weight?: number }) {
  const big = name === "x" && size <= 11 ? null : BIG[name];
  if (big)
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        aria-hidden="true"
        style={name === "chevr" ? { transform: "rotate(-90deg)" } : undefined}
        // biome-ignore lint/security/noDangerouslySetInnerHtml: fixed icon markup from this file
        dangerouslySetInnerHTML={{ __html: BODIES[big] }}
      />
    );
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={weight}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      // biome-ignore lint/security/noDangerouslySetInnerHtml: fixed icon markup from this file
      dangerouslySetInnerHTML={{ __html: (SMALL as Record<string, string>)[name] ?? "" }}
    />
  );
}
