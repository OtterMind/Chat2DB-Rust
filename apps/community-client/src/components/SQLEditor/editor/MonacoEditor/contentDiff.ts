import { ContentDiffKind } from '../../type';

type ContentDiffHunkKind = ContentDiffKind.Added | ContentDiffKind.Deleted | ContentDiffKind.Modified;

interface ContentDiffRange {
  startLineNumber: number;
  endLineNumber: number;
}

export interface ContentDiffHunk {
  id: string;
  kind: ContentDiffHunkKind;
  baselineRange: ContentDiffRange;
  currentRange: ContentDiffRange;
  baselineLineCount: number;
  currentLineCount: number;
  displayLineCount: number;
  baselineText: string;
  currentText: string;
  anchorLineNumber: number;
}

type DiffOp =
  | {
      kind: ContentDiffKind.Equal;
      baselineLine: number;
      currentLine: number;
      text: string;
    }
  | {
      kind: ContentDiffKind.Deleted;
      baselineLine: number;
      text: string;
    }
  | {
      kind: ContentDiffKind.Added;
      currentLine: number;
      text: string;
    };

const MAX_DIFF_MATRIX_CELLS = 250000;

export const buildContentDiffHunks = (baselineText: string, currentText: string): ContentDiffHunk[] => {
  if (baselineText === currentText) {
    return [];
  }

  const baselineLines = splitLines(baselineText);
  const currentLines = splitLines(currentText);

  if (!baselineLines.length && currentLines.length) {
    return [
      createHunk({
        kind: ContentDiffKind.Added,
        baselineStartLine: 1,
        currentStartLine: 1,
        baselineLines: [],
        currentLines,
        currentLineCount: currentLines.length,
      }),
    ];
  }

  if (baselineLines.length && !currentLines.length) {
    return [
      createHunk({
        kind: ContentDiffKind.Deleted,
        baselineStartLine: 1,
        currentStartLine: 1,
        baselineLines,
        currentLines: [],
        currentLineCount: 0,
      }),
    ];
  }

  if (baselineLines.length * currentLines.length > MAX_DIFF_MATRIX_CELLS) {
    return [
      createHunk({
        kind: ContentDiffKind.Modified,
        baselineStartLine: 1,
        currentStartLine: 1,
        baselineLines,
        currentLines,
        currentLineCount: currentLines.length,
      }),
    ];
  }

  return compactDiffOps(buildDiffOps(baselineLines, currentLines), currentLines.length);
};

const splitLines = (text: string) => {
  if (!text) {
    return [];
  }

  return text.split(/\r\n|\r|\n/);
};

const buildDiffOps = (baselineLines: string[], currentLines: string[]): DiffOp[] => {
  const baselineCount = baselineLines.length;
  const currentCount = currentLines.length;
  const table = Array.from({ length: baselineCount + 1 }, () => Array(currentCount + 1).fill(0));

  for (let baselineIndex = 1; baselineIndex <= baselineCount; baselineIndex += 1) {
    for (let currentIndex = 1; currentIndex <= currentCount; currentIndex += 1) {
      if (baselineLines[baselineIndex - 1] === currentLines[currentIndex - 1]) {
        table[baselineIndex][currentIndex] = table[baselineIndex - 1][currentIndex - 1] + 1;
      } else {
        table[baselineIndex][currentIndex] = Math.max(
          table[baselineIndex - 1][currentIndex],
          table[baselineIndex][currentIndex - 1],
        );
      }
    }
  }

  const ops: DiffOp[] = [];
  let baselineIndex = baselineCount;
  let currentIndex = currentCount;

  while (baselineIndex > 0 || currentIndex > 0) {
    if (baselineIndex > 0 && currentIndex > 0 && baselineLines[baselineIndex - 1] === currentLines[currentIndex - 1]) {
      ops.push({
        kind: ContentDiffKind.Equal,
        baselineLine: baselineIndex,
        currentLine: currentIndex,
        text: baselineLines[baselineIndex - 1],
      });
      baselineIndex -= 1;
      currentIndex -= 1;
      continue;
    }

    if (
      currentIndex > 0 &&
      (baselineIndex === 0 || table[baselineIndex][currentIndex - 1] >= table[baselineIndex - 1][currentIndex])
    ) {
      ops.push({
        kind: ContentDiffKind.Added,
        currentLine: currentIndex,
        text: currentLines[currentIndex - 1],
      });
      currentIndex -= 1;
      continue;
    }

    ops.push({
      kind: ContentDiffKind.Deleted,
      baselineLine: baselineIndex,
      text: baselineLines[baselineIndex - 1],
    });
    baselineIndex -= 1;
  }

  return ops.reverse();
};

const compactDiffOps = (ops: DiffOp[], currentLineCount: number): ContentDiffHunk[] => {
  const hunks: ContentDiffHunk[] = [];
  let baselineCursor = 1;
  let currentCursor = 1;
  let baselineStartLine = 1;
  let currentStartLine = 1;
  let deletedLines: string[] = [];
  let addedLines: string[] = [];
  let trailingBlankContextLineCount = 0;

  const flush = () => {
    if (!deletedLines.length && !addedLines.length) {
      return;
    }

    const kind: ContentDiffHunkKind =
      deletedLines.length && addedLines.length
        ? ContentDiffKind.Modified
        : deletedLines.length
        ? ContentDiffKind.Deleted
        : ContentDiffKind.Added;

    hunks.push(
      createHunk({
        kind,
        baselineStartLine,
        currentStartLine,
        baselineLines: deletedLines,
        currentLines: addedLines,
        currentLineCount,
        displayLineCount: getDisplayLineCount(kind, deletedLines, addedLines, trailingBlankContextLineCount),
      }),
    );

    deletedLines = [];
    addedLines = [];
    trailingBlankContextLineCount = 0;
  };

  ops.forEach((op) => {
    if (op.kind === ContentDiffKind.Equal) {
      if (deletedLines.length && isBlankLine(op.text)) {
        trailingBlankContextLineCount += 1;
        baselineCursor += 1;
        currentCursor += 1;
        return;
      }

      flush();
      baselineCursor += 1;
      currentCursor += 1;
      baselineStartLine = baselineCursor;
      currentStartLine = currentCursor;
      return;
    }

    if (!deletedLines.length && !addedLines.length) {
      baselineStartLine = baselineCursor;
      currentStartLine = currentCursor;
    }

    if (op.kind === ContentDiffKind.Deleted) {
      deletedLines.push(op.text);
      baselineCursor += 1;
      trailingBlankContextLineCount = 0;
      return;
    }

    addedLines.push(op.text);
    currentCursor += 1;
    trailingBlankContextLineCount = 0;
  });

  flush();

  return hunks;
};

const createHunk = (params: {
  kind: ContentDiffHunkKind;
  baselineStartLine: number;
  currentStartLine: number;
  baselineLines: string[];
  currentLines: string[];
  currentLineCount: number;
  displayLineCount?: number;
}): ContentDiffHunk => {
  const { kind, baselineStartLine, currentStartLine, baselineLines, currentLines, currentLineCount, displayLineCount } =
    params;
  const baselineEndLine = Math.max(baselineStartLine, baselineStartLine + baselineLines.length - 1);
  const currentEndLine = Math.max(currentStartLine, currentStartLine + currentLines.length - 1);
  const anchorLineNumber = clamp(currentStartLine, 1, Math.max(1, currentLineCount));
  const baselineText = baselineLines.join('\n');
  const currentText = currentLines.join('\n');

  return {
    id: `${kind}:${baselineStartLine}:${baselineEndLine}:${currentStartLine}:${currentEndLine}:${hashText(
      `${baselineText}\n---\n${currentText}`,
    )}`,
    kind,
    baselineRange: {
      startLineNumber: baselineStartLine,
      endLineNumber: baselineEndLine,
    },
    currentRange: {
      startLineNumber: kind === ContentDiffKind.Deleted ? anchorLineNumber : currentStartLine,
      endLineNumber: kind === ContentDiffKind.Deleted ? anchorLineNumber : currentEndLine,
    },
    baselineLineCount: baselineLines.length,
    currentLineCount: currentLines.length,
    displayLineCount: displayLineCount ?? Math.max(baselineLines.length, currentLines.length, 1),
    baselineText,
    currentText,
    anchorLineNumber,
  };
};

const getDisplayLineCount = (
  kind: ContentDiffHunkKind,
  deletedLines: string[],
  addedLines: string[],
  trailingBlankContextLineCount: number,
) => {
  const changedLineCount =
    kind === ContentDiffKind.Added ? addedLines.length : Math.max(deletedLines.length, addedLines.length, 1);

  return changedLineCount + trailingBlankContextLineCount;
};

const isBlankLine = (line: string) => !line.trim();

const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max);

const hashText = (value: string) => {
  let hash = 0;

  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) | 0;
  }

  return Math.abs(hash).toString(36);
};
