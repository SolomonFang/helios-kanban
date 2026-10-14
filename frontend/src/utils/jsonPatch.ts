import { applyPatch, type Operation } from 'rfc6902';

type EntriesContainer = { entries: unknown[] };

function isEntriesContainer(target: object): target is EntriesContainer {
  return (
    'entries' in target && Array.isArray((target as EntriesContainer).entries)
  );
}

function parseEntryIndex(path: string): number | null {
  const match = path.match(/^\/entries\/(\d+)$/);
  if (!match) return null;
  const index = Number(match[1]);
  return Number.isNaN(index) ? null : index;
}

/** Ensure `entries` is long enough to apply an add/replace at `index`. */
function padEntriesArray(entries: unknown[], index: number): void {
  while (entries.length < index) {
    entries.push(null);
  }
}

export function applyUpsertPatch(target: object, ops: Operation[]): void {
  ops.forEach((op) => {
    const index = parseEntryIndex(op.path);

    if (index !== null && isEntriesContainer(target) && op.op === 'add') {
      padEntriesArray(target.entries, index);
    }

    let [error] = applyPatch(target, [op]);

    if (error?.name === 'MissingError') {
      if (op.op === 'replace') {
        applyPatch(target, [{ ...op, op: 'add' }]);
        return;
      }

      if (op.op === 'add' && index !== null && isEntriesContainer(target)) {
        padEntriesArray(target.entries, index);
        [error] = applyPatch(target, [op]);
        if (error) {
          applyPatch(target, [{ ...op, op: 'replace' }]);
        }
      }
    }
  });
}
