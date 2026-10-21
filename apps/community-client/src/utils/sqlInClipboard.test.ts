import { parseClipboardTextToSqlInTokens } from './sqlInClipboard';

function assertEqual(actual: unknown, expected: unknown, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

assertEqual(parseClipboardTextToSqlInTokens('a,b,c'), ['a', 'b', 'c'], 'parses comma-separated text');
assertEqual(parseClipboardTextToSqlInTokens('a\nb\nc'), ['a', 'b', 'c'], 'parses newline-separated text');
assertEqual(parseClipboardTextToSqlInTokens('a\tb\nc\td'), ['a', 'b', 'c', 'd'], 'parses TSV grid row-major');
assertEqual(parseClipboardTextToSqlInTokens(' a , , b '), ['a', 'b'], 'trims and drops empty tokens');
assertEqual(parseClipboardTextToSqlInTokens('"a,b",c'), ['a,b', 'c'], 'keeps comma inside quoted CSV value');
assertEqual(parseClipboardTextToSqlInTokens('"a""b",c'), ['a"b', 'c'], 'unescapes doubled quotes in CSV value');
assertEqual(parseClipboardTextToSqlInTokens('"a\nb",c'), ['a\nb', 'c'], 'keeps newline inside quoted CSV value');
assertEqual(parseClipboardTextToSqlInTokens('a," b ",c'), ['a', 'b', 'c'], 'trims quoted values after parsing');
assertEqual(parseClipboardTextToSqlInTokens('a,b,'), ['a', 'b'], 'drops empty trailing comma token');
assertEqual(parseClipboardTextToSqlInTokens('"a\tb"\tc'), ['a\tb', 'c'], 'keeps tab inside quoted TSV value');
assertEqual(
  parseClipboardTextToSqlInTokens('"a,b"\n"c,d"'),
  ['a,b', 'c,d'],
  'uses newline when commas are quoted only',
);
assertEqual(parseClipboardTextToSqlInTokens('"a,b'), ['a,b'], 'handles unmatched quote as one value');
assertEqual(parseClipboardTextToSqlInTokens('a"b,c'), ['a"b', 'c'], 'keeps quote inside unquoted value');
assertEqual(parseClipboardTextToSqlInTokens(''), [], 'returns empty tokens for blank text');

console.log('sqlInClipboard tests passed');
