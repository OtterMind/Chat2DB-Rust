import {
  normalizeSqlInputPunctuation,
  shouldNormalizeSqlInputPunctuation,
  sqlPunctuationFromKeyboardEvent,
} from './sqlInputPunctuation';

function assertEqual(actual: any, expected: any, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

function keyboardEvent(code: string, key: string, shiftKey = false, extra: Partial<KeyboardEvent> = {}) {
  return {
    altKey: false,
    code,
    ctrlKey: false,
    isComposing: false,
    key,
    metaKey: false,
    shiftKey,
    ...extra,
  } as KeyboardEvent;
}

assertEqual(sqlPunctuationFromKeyboardEvent(keyboardEvent('Period', '。')), '.', 'Chinese period key inserts ASCII dot');
assertEqual(sqlPunctuationFromKeyboardEvent(keyboardEvent('Comma', '，')), ',', 'Chinese comma key inserts ASCII comma');
assertEqual(sqlPunctuationFromKeyboardEvent(keyboardEvent('Semicolon', '；')), ';', 'Chinese semicolon key inserts ASCII semicolon');
assertEqual(sqlPunctuationFromKeyboardEvent(keyboardEvent('Digit9', 'Process', true)), '(', 'IME shift digit 9 inserts ASCII left paren');
assertEqual(sqlPunctuationFromKeyboardEvent(keyboardEvent('Digit0', 'Process', true)), ')', 'IME shift digit 0 inserts ASCII right paren');
assertEqual(sqlPunctuationFromKeyboardEvent(keyboardEvent('Period', 'Process', true)), '>', 'IME shift period inserts ASCII greater-than');
assertEqual(sqlPunctuationFromKeyboardEvent(keyboardEvent('NumpadDecimal', 'Unidentified')), '.', 'numpad decimal inserts ASCII dot during IME');
assertEqual(sqlPunctuationFromKeyboardEvent(keyboardEvent('Period', '.')), null, 'plain ASCII period is handled by Monaco');
assertEqual(sqlPunctuationFromKeyboardEvent(keyboardEvent('Period', '。', false, { metaKey: true })), null, 'meta shortcut is ignored');
assertEqual(sqlPunctuationFromKeyboardEvent(keyboardEvent('Period', '。', false, { isComposing: true })), null, 'composition is ignored');
assertEqual(sqlPunctuationFromKeyboardEvent(keyboardEvent('KeyA', 'Process')), null, 'letters are not intercepted');

assertEqual(normalizeSqlInputPunctuation('。'), '.', 'Chinese period normalizes to ASCII dot');
assertEqual(normalizeSqlInputPunctuation('，'), ',', 'Chinese comma normalizes to ASCII comma');
assertEqual(normalizeSqlInputPunctuation('；'), ';', 'Chinese semicolon normalizes to ASCII semicolon');
assertEqual(normalizeSqlInputPunctuation('：'), ':', 'Chinese colon normalizes to ASCII colon');
assertEqual(normalizeSqlInputPunctuation('（'), '(', 'fullwidth left paren normalizes');
assertEqual(normalizeSqlInputPunctuation('）'), ')', 'fullwidth right paren normalizes');
assertEqual(normalizeSqlInputPunctuation('a'), null, 'letters are not normalized');
assertEqual(normalizeSqlInputPunctuation('。，'), null, 'multi-character text is not normalized');

function assertNormalizeContext(markedSql: string, expected: boolean, message: string) {
  const offset = markedSql.indexOf('|');
  const sql = markedSql.replace('|', '');
  assertEqual(shouldNormalizeSqlInputPunctuation(sql, offset), expected, message);
}

assertNormalizeContext('SELECT a| FROM t', true, 'SQL structure position normalizes punctuation');
assertNormalizeContext("SELECT '你好|世界'", false, 'single quote string preserves Chinese punctuation');
assertNormalizeContext("SELECT 'it''s |ok'", false, 'escaped single quote stays inside string');
assertNormalizeContext("SELECT 'done' |FROM t", true, 'position after single quote string normalizes punctuation');
assertNormalizeContext('SELECT "中文|列" FROM t', false, 'double quote identifier preserves Chinese punctuation');
assertNormalizeContext('SELECT `中文|列` FROM t', false, 'backtick identifier preserves Chinese punctuation');
assertNormalizeContext('SELECT [中文|列] FROM t', false, 'bracket identifier preserves Chinese punctuation');
assertNormalizeContext('SELECT 1 -- 中文|注释', false, 'line comment preserves Chinese punctuation');
assertNormalizeContext('SELECT 1 -- 中文注释\nWHERE a|=1', true, 'position after line comment normalizes punctuation');
assertNormalizeContext('SELECT /* 中文|注释 */ 1', false, 'block comment preserves Chinese punctuation');
assertNormalizeContext('SELECT /* 中文注释 */ a| FROM t', true, 'position after block comment normalizes punctuation');

console.log('sqlInputPunctuation tests passed');
