/** A release's Markdown notes as sections of plain lines: a `#`–`####` heading starts one, `- `/`* `
 * lines are its items (bold, code marks and link targets dropped); other text is left out.
 * Pure, for scripts/test-news.mjs. */
export function releaseNotes(md: string): { title: string; lines: string[] }[] {
  const out: { title: string; lines: string[] }[] = [];
  const plain = (s: string) => s.replace(/\*\*|__|`/g, "").replace(/\[([^\]]+)\]\([^)]*\)/g, "$1").trim();
  for (const raw of md.split(/\r?\n/)) {
    const line = raw.trim();
    const head = /^#{1,4}\s+(.+)$/.exec(line);
    const item = /^[-*]\s+(.+)$/.exec(line);
    if (head) out.push({ title: plain(head[1]), lines: [] });
    else if (item) {
      if (!out.length) out.push({ title: "", lines: [] });
      out[out.length - 1].lines.push(plain(item[1]));
    }
  }
  return out.filter((s) => s.lines.length);
}
