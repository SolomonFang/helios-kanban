/**
 * Streaming-safe markdown repair (remend-style).
 *
 * While an assistant message is streaming, markdown syntax arrives
 * incomplete (`**bold` without the closing `**`, an unterminated code
 * fence...). Parsing that as-is flashes raw syntax to the user. This pass
 * auto-closes incomplete constructs on a COPY of the string before parsing;
 * the source content is never modified.
 *
 * Priority order mirrors what is most visually jarring when broken:
 *   1. code fences (and while inside a fence, skip all inline repairs —
 *      `**` or backticks in code are literals, not markdown)
 *   2. bold `**`
 *   3. inline code backticks
 *   4. unclosed trailing links `[text](url`
 */
export function repairMarkdown(text: string): string {
  if (!text) return text;

  // 1. Unclosed code fence: close it and stop — inside a fence nothing else
  // is markdown, so inline repairs must not run.
  let fenceCount = 0;
  for (const line of text.split('\n')) {
    if (line.trimStart().startsWith('```')) fenceCount++;
  }
  if (fenceCount % 2 === 1) {
    return text + '\n```';
  }

  let result = text;

  // 2. Unclosed bold (odd number of `**` markers)
  const boldMarkers = result.match(/\*\*/g);
  if (boldMarkers && boldMarkers.length % 2 === 1) {
    result += '**';
  }

  // 3. Unclosed inline code (odd number of single backticks, excluding
  // backticks that are part of a fence, which step 1 already ruled out)
  const singleBackticks = result.match(/(?<!`)`(?!`)/g);
  if (singleBackticks && singleBackticks.length % 2 === 1) {
    result += '`';
  }

  // 4. Unclosed trailing link: `[text](url` at end of string → close paren
  if (/\[[^\]]*\]\([^)]*$/.test(result)) {
    result += ')';
  }

  return result;
}
