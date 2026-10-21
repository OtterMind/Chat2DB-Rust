export function parseClipboardTextToSqlInTokens(text: string): string[] {
  if (!text) {
    return [];
  }

  const normalized = text
    .replace(/\r\n/g, '\n')
    .replace(/\r/g, '\n')
    .trim();
  if (!normalized) {
    return [];
  }

  return splitClipboardText(normalized)
    .map((token) => token.trim())
    .filter((token) => token.length > 0);
}

function splitClipboardText(text: string): string[] {
  const delimiter = detectDelimiter(text);
  return parseDelimitedText(text, delimiter);
}

function detectDelimiter(text: string): ',' | '\t' | '\n' {
  let hasComma = false;
  let hasTab = false;
  let inQuotes = false;

  for (let i = 0; i < text.length; i++) {
    const char = text[i];
    if (char === '"' && (inQuotes || isAtQuotedValueStart(text, i))) {
      if (inQuotes && text[i + 1] === '"') {
        i += 1;
      } else {
        inQuotes = !inQuotes;
      }
      continue;
    }
    if (inQuotes) {
      continue;
    }
    if (char === '\t') {
      hasTab = true;
    } else if (char === ',') {
      hasComma = true;
    }
  }

  if (hasTab) {
    return '\t';
  }
  if (hasComma) {
    return ',';
  }
  return '\n';
}

function parseDelimitedText(text: string, delimiter: ',' | '\t' | '\n'): string[] {
  const values: string[] = [];
  let current = '';
  let inQuotes = false;

  for (let i = 0; i < text.length; i++) {
    const char = text[i];
    if (char === '"' && (inQuotes || isAtQuotedTokenStart(current))) {
      if (inQuotes && text[i + 1] === '"') {
        current += '"';
        i += 1;
      } else {
        inQuotes = !inQuotes;
      }
      continue;
    }

    if (!inQuotes && isTokenSeparator(char, delimiter)) {
      values.push(current);
      current = '';
      continue;
    }

    current += char;
  }

  values.push(current);
  return values;
}

function isAtQuotedValueStart(text: string, quoteIndex: number) {
  for (let i = quoteIndex - 1; i >= 0; i--) {
    const char = text[i];
    if (char === ',' || char === '\t' || char === '\n') {
      return true;
    }
    if (char !== ' ') {
      return false;
    }
  }
  return true;
}

function isAtQuotedTokenStart(current: string) {
  return current.trim().length === 0;
}

function isTokenSeparator(char: string, delimiter: ',' | '\t' | '\n') {
  if (char === '\n') {
    return true;
  }
  return char === delimiter;
}
