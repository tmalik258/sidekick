// Secrets (API keys, tokens, private keys, passwords) shown as dots in
// transcripts and history. The same patterns as `classify::mask` in Rust.

const SECRET = new RegExp(
  [
    "AKIA[0-9A-Z]{16}",
    "sk-(?:ant-|proj-)?[A-Za-z0-9_\\-]{20,}",
    "gh[pousr]_[A-Za-z0-9]{30,}",
    "xox[abpr]-[A-Za-z0-9\\-]{10,}",
    "-----BEGIN [A-Z ]*PRIVATE KEY-----[\\s\\S]*?(?:-----END [A-Z ]*PRIVATE KEY-----|$)",
    "eyJ[A-Za-z0-9_\\-]{10,}\\.[A-Za-z0-9_\\-]{10,}\\.[A-Za-z0-9_\\-]{10,}",
    "((?:[Pp][Aa][Ss][Ss][Ww](?:[Oo][Rr])?[Dd]|[Ss][Ee][Cc][Rr][Ee][Tt]|[Aa][Pp][Ii][_-]?[Kk][Ee][Yy]|[Aa][Cc][Cc][Ee][Ss][Ss][_-]?[Tt][Oo][Kk][Ee][Nn]|[Aa][Uu][Tt][Hh][_-]?[Tt][Oo][Kk][Ee][Nn]|[Cc][Ll][Ii][Ee][Nn][Tt][_-]?[Ss][Ee][Cc][Rr][Ee][Tt])[\"']?\\s*[:=]\\s*[\"']?)[^\\s\"',;]{6,}",
  ].join("|"),
  "g",
);

export const MASKED = "••••";

export function mask(text: string): string {
  return text.replace(SECRET, (_m, key?: string) => (key ? `${key}${MASKED}` : MASKED));
}
