export interface InternalResultGridClipboard {
  text: string;
  rows: string[][];
}

let internalResultGridClipboard: InternalResultGridClipboard | null = null;

function serializeGrid(rows: string[][]) {
  return rows.map((row) => row.join('\t')).join('\n');
}

export function clearInternalClipboard() {
  internalResultGridClipboard = null;
}

export function setInternalResultGridClipboard(rows: string[][]) {
  internalResultGridClipboard = {
    text: serializeGrid(rows),
    rows: rows.map((row) => [...row]),
  };
}

export function getInternalResultGridClipboard(text: string) {
  if (internalResultGridClipboard?.text !== text) {
    return null;
  }
  return internalResultGridClipboard.rows.map((row) => [...row]);
}
