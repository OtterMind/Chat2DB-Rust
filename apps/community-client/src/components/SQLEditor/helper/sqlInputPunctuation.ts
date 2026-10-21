const normalPunctuationByCode: Record<string, string> = {
  Backquote: '`',
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  IntlBackslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
  NumpadAdd: '+',
  NumpadSubtract: '-',
  NumpadMultiply: '*',
  NumpadDivide: '/',
  NumpadDecimal: '.',
  NumpadComma: ',',
  NumpadEqual: '=',
};

const shiftPunctuationByCode: Record<string, string> = {
  Backquote: '~',
  Digit1: '!',
  Digit2: '@',
  Digit3: '#',
  Digit4: '$',
  Digit5: '%',
  Digit6: '^',
  Digit7: '&',
  Digit8: '*',
  Digit9: '(',
  Digit0: ')',
  Minus: '_',
  Equal: '+',
  BracketLeft: '{',
  BracketRight: '}',
  Backslash: '|',
  IntlBackslash: '|',
  Semicolon: ':',
  Quote: '"',
  Comma: '<',
  Period: '>',
  Slash: '?',
};

const normalizedSqlPunctuation: Record<string, string> = {
  '。': '.',
  '．': '.',
  '｡': '.',
  '，': ',',
  '､': ',',
  '；': ';',
  '：': ':',
  '（': '(',
  '）': ')',
  '［': '[',
  '］': ']',
  '【': '[',
  '】': ']',
  '｛': '{',
  '｝': '}',
  '、': '/',
  '／': '/',
  '＼': '\\',
  '｀': '`',
  '～': '~',
  '！': '!',
  '？': '?',
  '＠': '@',
  '＃': '#',
  '＄': '$',
  '％': '%',
  '＾': '^',
  '＆': '&',
  '＊': '*',
  '－': '-',
  '＿': '_',
  '＝': '=',
  '＋': '+',
  '＜': '<',
  '＞': '>',
  '｜': '|',
  '＂': '"',
  '“': '"',
  '”': '"',
  '＇': "'",
  '‘': "'",
  '’': "'",
};

type KeyboardLikeEvent = Pick<
  KeyboardEvent,
  'altKey' | 'code' | 'ctrlKey' | 'isComposing' | 'key' | 'metaKey' | 'shiftKey'
>;

type SqlPunctuationContext =
  | 'sql'
  | 'singleQuoteString'
  | 'doubleQuoteString'
  | 'backtickIdentifier'
  | 'bracketIdentifier'
  | 'lineComment'
  | 'blockComment';

export function sqlPunctuationFromKeyboardEvent(event: KeyboardLikeEvent | null | undefined) {
  if (!event || event.metaKey || event.ctrlKey || event.altKey || event.isComposing) {
    return null;
  }

  const normalizedKey = normalizeSqlInputPunctuation(event.key);
  if (normalizedKey) {
    return normalizedKey;
  }

  if (event.key !== 'Process' && event.key !== 'Unidentified' && event.key !== 'Dead') {
    return null;
  }

  const punctuation = event.shiftKey ? shiftPunctuationByCode[event.code] : normalPunctuationByCode[event.code];
  return punctuation || null;
}

export function normalizeSqlInputPunctuation(text: string | null | undefined) {
  if (!text || text.length !== 1) {
    return null;
  }

  const normalized = normalizedSqlPunctuation[text];
  return normalized && normalized !== text ? normalized : null;
}

export function shouldNormalizeSqlInputPunctuation(sql: string | null | undefined, offset: number) {
  return sqlPunctuationContextAtOffset(sql || '', offset) === 'sql';
}

function sqlPunctuationContextAtOffset(sql: string, offset: number): SqlPunctuationContext {
  const safeOffset = Math.max(0, Math.min(offset, sql.length));
  let context: SqlPunctuationContext = 'sql';

  for (let index = 0; index < safeOffset; index += 1) {
    const char = sql[index];
    const nextChar = sql[index + 1];

    if (context === 'sql') {
      if (char === "'") {
        context = 'singleQuoteString';
      } else if (char === '"') {
        context = 'doubleQuoteString';
      } else if (char === '`') {
        context = 'backtickIdentifier';
      } else if (char === '[') {
        context = 'bracketIdentifier';
      } else if (char === '-' && nextChar === '-') {
        context = 'lineComment';
        index += 1;
      } else if (char === '/' && nextChar === '*') {
        context = 'blockComment';
        index += 1;
      }
      continue;
    }

    if (context === 'singleQuoteString') {
      if (char === "'" && nextChar === "'") {
        index += 1;
      } else if (char === "'" && !isBackslashEscaped(sql, index)) {
        context = 'sql';
      }
      continue;
    }

    if (context === 'doubleQuoteString') {
      if (char === '"' && nextChar === '"') {
        index += 1;
      } else if (char === '"' && !isBackslashEscaped(sql, index)) {
        context = 'sql';
      }
      continue;
    }

    if (context === 'backtickIdentifier') {
      if (char === '`' && nextChar === '`') {
        index += 1;
      } else if (char === '`') {
        context = 'sql';
      }
      continue;
    }

    if (context === 'bracketIdentifier') {
      if (char === ']' && nextChar === ']') {
        index += 1;
      } else if (char === ']') {
        context = 'sql';
      }
      continue;
    }

    if (context === 'lineComment') {
      if (char === '\n' || char === '\r') {
        context = 'sql';
      }
      continue;
    }

    if (context === 'blockComment' && char === '*' && nextChar === '/') {
      context = 'sql';
      index += 1;
    }
  }

  return context;
}

function isBackslashEscaped(sql: string, index: number) {
  let backslashCount = 0;
  for (let cursor = index - 1; cursor >= 0 && sql[cursor] === '\\'; cursor -= 1) {
    backslashCount += 1;
  }
  return backslashCount % 2 === 1;
}
