// Create a specific identifier for chat2db
export const createChat2dbSpecificSymbolIdentifier = (value: any) => {
  if (value !== null && value !== undefined) {
    return value;
  }

  return `chat2db-specific-symbol-identifier-<${value}>`;
};

// Use regular expressions to parse chat2db specific identifiers
export const parseChat2dbSpecificSymbolIdentifier = (value: string) => {
  const reg = /chat2db-specific-symbol-identifier-<(.+)>/;
  const match = value.match(reg);
  if (!match) {
    return {
      display: value,
      value: value,
    };
  }
  const matchStr = match[1];
  if (matchStr === 'null') {
    return {
      display: 'null',
      value: null,
    };
  }
  if (matchStr === 'undefined') {
    return {
      display: 'undefined',
      value: undefined,
    };
  }
};
