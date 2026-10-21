import { TIP_TYPE } from '../type';
import { ISqlEditorHintVO } from '@/typings/sqlParser';
import { TextPosition, TextRange } from './insertValueHighlight';

export interface RoutineParameterHintPayload {
  routineName?: string;
  routineType?: TIP_TYPE;
  detail?: string;
  insertText?: string;
}

export interface RoutineParameterHintItem {
  parameterIndex: number;
  parameterName: string;
  parameterType?: string;
  label: string;
  range: TextRange;
  active: boolean;
}

export interface RoutineParameterHintContext {
  routineName?: string;
  routineType?: TIP_TYPE;
  hints: RoutineParameterHintItem[];
  anchorRange: TextRange;
}

interface ParsedRoutineParameter {
  parameterName: string;
  parameterType?: string;
}

export function isRoutineParameterHintPayload(value: unknown): value is RoutineParameterHintPayload {
  if (!value || typeof value !== 'object') {
    return false;
  }
  const payload = value as RoutineParameterHintPayload;
  return !!parseRoutineParameterDetail(payload.detail).length;
}

export function parseRoutineParameterDetail(detail: string | null | undefined): ParsedRoutineParameter[] {
  const normalizedDetail = stripOuterParentheses((detail || '').trim());
  if (!normalizedDetail) {
    return [];
  }

  return splitTopLevel(normalizedDetail, 0, normalizedDetail.length)
    .map(parseRoutineParameterSegment)
    .filter((parameter): parameter is ParsedRoutineParameter => !!parameter);
}

export function getRoutineParameterHintContext(
  payload: RoutineParameterHintPayload | null | undefined,
  cursorPosition: TextPosition | null | undefined,
  getLineContent?: (lineNumber: number) => string | null | undefined,
): RoutineParameterHintContext | null {
  if (!payload || !cursorPosition) {
    return null;
  }

  const parameters = parseRoutineParameterDetail(payload.detail);
  if (!parameters.length) {
    return null;
  }

  const lineContent = getLineContent?.(cursorPosition.lineNumber) || '';
  const callRange = lineContent ? getRoutineCallRange(lineContent, cursorPosition.column, payload.routineName) : null;
  if (lineContent && !callRange) {
    return null;
  }

  const argumentRanges = callRange
    ? getRoutineArgumentRanges(lineContent, cursorPosition.lineNumber, callRange.openIndex, callRange.closeIndex)
    : [];
  const fallbackRange = argumentRanges[0] || collapsedRange(cursorPosition);
  const activeIndex = getActiveArgumentIndex(cursorPosition, argumentRanges, parameters.length);
  const anchorRange = argumentRanges[activeIndex] || fallbackRange;

  return {
    routineName: payload.routineName,
    routineType: payload.routineType,
    anchorRange,
    hints: parameters.map((parameter, index) => {
      const range = argumentRanges[index] || fallbackRange;
      return {
        parameterIndex: index,
        parameterName: parameter.parameterName,
        parameterType: parameter.parameterType,
        label: parameter.parameterType
          ? `${parameter.parameterName}:${parameter.parameterType}`
          : parameter.parameterName,
        range,
        active: index === activeIndex,
      };
    }),
  };
}

export function routineParameterHintContextFromEditorHint(
  editorHint: ISqlEditorHintVO | null | undefined,
): RoutineParameterHintContext | null {
  if (!editorHint || editorHint.type !== 'ROUTINE_PARAMETER' || !editorHint.items?.length) {
    return null;
  }

  const valueRange = toTextRange(editorHint.valueRange);
  const hints = editorHint.items
    .map((item): RoutineParameterHintItem | null => {
      const range = toTextRange(item.range) || valueRange;
      const parameterName = item.fieldName || item.label;
      if (!parameterName || !range) {
        return null;
      }
      const parameterType = item.fieldType || undefined;
      return {
        parameterIndex: toFiniteNumber(item.columnIndex, 0),
        parameterName,
        ...(parameterType ? { parameterType } : {}),
        label: item.label || (parameterType ? `${parameterName}:${parameterType}` : parameterName),
        range,
        active: !!item.active,
      };
    })
    .filter((item): item is RoutineParameterHintItem => !!item);

  if (!hints.length) {
    return null;
  }

  const activeHint = hints.find((item) => item.active) || hints[0];
  return {
    hints,
    anchorRange: valueRange || activeHint.range,
  };
}

function parseRoutineParameterSegment(segment: string): ParsedRoutineParameter | null {
  const trimmedSegment = segment.trim();
  if (!trimmedSegment) {
    return null;
  }

  const colonIndex = trimmedSegment.indexOf(':');
  if (colonIndex >= 0) {
    const parameterName = trimmedSegment.slice(0, colonIndex).trim();
    const parameterType = trimmedSegment.slice(colonIndex + 1).trim();
    if (!parameterName) {
      return null;
    }
    return {
      parameterName,
      parameterType: parameterType || undefined,
    };
  }

  const [parameterName, ...typeParts] = trimmedSegment.split(/\s+/);
  if (!parameterName) {
    return null;
  }
  return {
    parameterName,
    parameterType: typeParts.join(' ') || undefined,
  };
}

function getRoutineCallRange(lineContent: string, cursorColumn: number, routineName?: string) {
  const cursorIndex = Math.max(0, cursorColumn - 1);
  const routineNames = getRoutineNameCandidates(routineName);
  const candidates: Array<{ nameIndex: number; openIndex: number; closeIndex: number }> = [];

  routineNames.forEach((name) => {
    const normalizedLine = lineContent.toLowerCase();
    const normalizedName = name.toLowerCase();
    let fromIndex = 0;
    while (fromIndex < normalizedLine.length) {
      const nameIndex = normalizedLine.indexOf(normalizedName, fromIndex);
      if (nameIndex < 0) {
        break;
      }
      const openIndex = getNextOpenParenthesisIndex(lineContent, nameIndex + name.length);
      if (openIndex >= 0) {
        const closeIndex = findMatchingParenthesis(lineContent, openIndex);
        if (closeIndex >= 0 && cursorIndex >= openIndex && cursorIndex <= closeIndex + 1) {
          candidates.push({ nameIndex, openIndex, closeIndex });
        }
      }
      fromIndex = nameIndex + normalizedName.length;
    }
  });

  if (candidates.length) {
    return candidates.sort((a, b) => b.nameIndex - a.nameIndex)[0];
  }

  return routineNames.length ? null : getNearestCallRange(lineContent, cursorIndex);
}

function getRoutineArgumentRanges(lineContent: string, lineNumber: number, openIndex: number, closeIndex: number) {
  if (closeIndex <= openIndex) {
    return [];
  }

  return splitTopLevelRanges(lineContent, openIndex + 1, closeIndex).map((range) => ({
    startLineNumber: lineNumber,
    startColumn: range.startIndex + 1,
    endLineNumber: lineNumber,
    endColumn: range.endIndex + 1,
  }));
}

function getActiveArgumentIndex(cursorPosition: TextPosition, argumentRanges: TextRange[], parameterCount: number) {
  const matchedIndex = argumentRanges.findIndex((range) => isPositionInRange(cursorPosition, range));
  if (matchedIndex >= 0) {
    return Math.min(matchedIndex, Math.max(0, parameterCount - 1));
  }

  const nextIndex = argumentRanges.findIndex(
    (range) =>
      comparePosition(cursorPosition.lineNumber, cursorPosition.column, range.startLineNumber, range.startColumn) < 0,
  );
  if (nextIndex >= 0) {
    return Math.min(nextIndex, Math.max(0, parameterCount - 1));
  }

  return Math.max(0, Math.min(argumentRanges.length - 1, parameterCount - 1));
}

function getRoutineNameCandidates(routineName?: string) {
  const candidates = [
    routineName,
    stripSqlIdentifierQuote(routineName),
    stripSqlIdentifierQuote(getLastNamePart(routineName)),
  ]
    .map((name) => (name || '').trim())
    .filter(Boolean);
  return Array.from(new Set(candidates));
}

function getLastNamePart(name?: string) {
  if (!name) {
    return '';
  }
  const parts = name.split('.');
  return parts[parts.length - 1];
}

function stripSqlIdentifierQuote(identifier?: string) {
  const trimmedIdentifier = (identifier || '').trim();
  if (
    (trimmedIdentifier.startsWith('`') && trimmedIdentifier.endsWith('`')) ||
    (trimmedIdentifier.startsWith('"') && trimmedIdentifier.endsWith('"')) ||
    (trimmedIdentifier.startsWith("'") && trimmedIdentifier.endsWith("'")) ||
    (trimmedIdentifier.startsWith('[') && trimmedIdentifier.endsWith(']'))
  ) {
    return trimmedIdentifier.slice(1, -1);
  }
  return trimmedIdentifier;
}

function getNextOpenParenthesisIndex(lineContent: string, fromIndex: number) {
  let index = fromIndex;
  while (index < lineContent.length && /\s/.test(lineContent[index])) {
    index += 1;
  }
  return lineContent[index] === '(' ? index : -1;
}

function getNearestCallRange(lineContent: string, cursorIndex: number) {
  for (let index = Math.min(cursorIndex, lineContent.length - 1); index >= 0; index -= 1) {
    if (lineContent[index] !== '(') {
      continue;
    }
    const closeIndex = findMatchingParenthesis(lineContent, index);
    if (closeIndex >= 0 && cursorIndex <= closeIndex + 1) {
      return { nameIndex: index, openIndex: index, closeIndex };
    }
  }
  return null;
}

function findMatchingParenthesis(text: string, openIndex: number) {
  let depth = 0;
  let quote: string | null = null;
  for (let index = openIndex; index < text.length; index += 1) {
    const char = text[index];
    if (quote) {
      if (char === '\\') {
        index += 1;
      } else if (char === quote) {
        if (text[index + 1] === quote) {
          index += 1;
        } else {
          quote = null;
        }
      }
      continue;
    }

    if (char === "'" || char === '"' || char === '`') {
      quote = char;
      continue;
    }
    if (char === '(') {
      depth += 1;
    } else if (char === ')') {
      depth -= 1;
      if (depth === 0) {
        return index;
      }
    }
  }
  return -1;
}

function splitTopLevelRanges(text: string, startIndex: number, endIndex: number) {
  const ranges: Array<{ startIndex: number; endIndex: number }> = [];
  let segmentStart = startIndex;
  splitTopLevel(text, startIndex, endIndex, (splitIndex) => {
    ranges.push(trimTextRange(text, segmentStart, splitIndex));
    segmentStart = splitIndex + 1;
  });
  ranges.push(trimTextRange(text, segmentStart, endIndex));
  return ranges.filter((range) => range.startIndex <= range.endIndex);
}

function splitTopLevel(text: string, startIndex: number, endIndex: number, onSplit?: (splitIndex: number) => void) {
  const segments: string[] = [];
  let depth = 0;
  let quote: string | null = null;
  let segmentStart = startIndex;

  for (let index = startIndex; index < endIndex; index += 1) {
    const char = text[index];
    if (quote) {
      if (char === '\\') {
        index += 1;
      } else if (char === quote) {
        if (text[index + 1] === quote) {
          index += 1;
        } else {
          quote = null;
        }
      }
      continue;
    }

    if (char === "'" || char === '"' || char === '`') {
      quote = char;
      continue;
    }
    if (char === '(' || char === '[') {
      depth += 1;
      continue;
    }
    if (char === ')' || char === ']') {
      depth = Math.max(0, depth - 1);
      continue;
    }
    if (char === ',' && depth === 0) {
      segments.push(text.slice(segmentStart, index));
      onSplit?.(index);
      segmentStart = index + 1;
    }
  }

  segments.push(text.slice(segmentStart, endIndex));
  return segments;
}

function stripOuterParentheses(detail: string) {
  if (!detail.startsWith('(') || !detail.endsWith(')')) {
    return detail;
  }
  const matchingIndex = findMatchingParenthesis(detail, 0);
  return matchingIndex === detail.length - 1 ? detail.slice(1, -1).trim() : detail;
}

function trimTextRange(text: string, startIndex: number, endIndex: number) {
  let start = startIndex;
  let end = endIndex;
  while (start < end && /\s/.test(text[start])) {
    start += 1;
  }
  while (end > start && /\s/.test(text[end - 1])) {
    end -= 1;
  }
  return { startIndex: start, endIndex: end };
}

function collapsedRange(position: TextPosition): TextRange {
  return {
    startLineNumber: position.lineNumber,
    startColumn: position.column,
    endLineNumber: position.lineNumber,
    endColumn: position.column,
  };
}

function toTextRange(range: ISqlEditorHintVO['rowRange']): TextRange | null {
  if (!range) {
    return null;
  }
  const textRange = {
    startLineNumber: toFiniteNumber(range.startLineNumber, 0),
    startColumn: toFiniteNumber(range.startColumn, 0),
    endLineNumber: toFiniteNumber(range.endLineNumber, 0),
    endColumn: toFiniteNumber(range.endColumn, 0),
  };
  return isValidRange(textRange) ? textRange : null;
}

function toFiniteNumber(value: number | string | null | undefined, fallback: number) {
  const numberValue = typeof value === 'number' ? value : Number(value);
  return Number.isFinite(numberValue) ? numberValue : fallback;
}

function isValidRange(range: TextRange) {
  return (
    range.startLineNumber > 0 &&
    range.startColumn > 0 &&
    range.endLineNumber > 0 &&
    range.endColumn > 0 &&
    comparePosition(range.startLineNumber, range.startColumn, range.endLineNumber, range.endColumn) <= 0
  );
}

function isPositionInRange(position: TextPosition, range: TextRange) {
  return (
    comparePosition(range.startLineNumber, range.startColumn, position.lineNumber, position.column) <= 0 &&
    comparePosition(position.lineNumber, position.column, range.endLineNumber, range.endColumn) <= 0
  );
}

function comparePosition(lineA: number, colA: number, lineB: number, colB: number) {
  if (lineA !== lineB) {
    return lineA - lineB;
  }
  return colA - colB;
}
