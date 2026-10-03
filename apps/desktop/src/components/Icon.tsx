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
  // Drawn for Sidekick (not from Solar), in the same 1.5 stroke style.
  plus: '<path fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5" d="M12 5v14M5 12h14"/>',
  ask: '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></g>',
  undo: '<path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M4 8h10.5a5.5 5.5 0 0 1 0 11H9M4 8l3.5-3.5M4 8l3.5 3.5"/>',
  screen:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5"><rect x="2.5" y="3.5" width="19" height="13" rx="2.5"/><path d="M8 20.5h8M12 16.5v4"/></g>',
  mic: '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"><rect x="8.5" y="2.5" width="7" height="12" rx="3.5"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3.5"/></g>',
  // Drawn for Sidekick (not from Solar), in the same 1.5 stroke style.
  history:
    '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.5"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.5 2"/></g>',
  // Drawn for Sidekick (not from Solar), in the same 1.5 stroke style.
  file: '<g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5"><path d="M14 2.5H7.5a2 2 0 0 0-2 2v15a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2V7L14 2.5Z"/><path d="M14 2.5V7h4.5M9 12.5h6M9 16h4"/></g>',
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
