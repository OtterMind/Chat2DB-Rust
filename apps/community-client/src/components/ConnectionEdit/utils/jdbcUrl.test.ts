import { formatJdbcHostForUrl, normalizeJdbcHostFromUrl, stripJdbcHostBrackets } from './jdbcUrl';

function assertEqual(actual: any, expected: any, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${expected}, got ${actual}`);
  }
}

const cases = [
  ['2001:db8::1002', '[2001:db8::1002]'],
  ['[2001:db8::1002]', '[2001:db8::1002]'],
  ['::1', '[::1]'],
  ['fe80::1%en0', '[fe80::1%en0]'],
  ['127.0.0.1', '127.0.0.1'],
  ['db.example.com', 'db.example.com'],
  ['http://localhost', 'http://localhost'],
  ['localhost:3306', 'localhost:3306'],
];

cases.forEach(([input, expected]) => {
  assertEqual(formatJdbcHostForUrl(input), expected, `formatJdbcHostForUrl(${input})`);
});

const oceanBaseUrl = `jdbc:oceanbase://${formatJdbcHostForUrl('2001:db8::1')}:2883/test`;
assertEqual(oceanBaseUrl, 'jdbc:oceanbase://[2001:db8::1]:2883/test', 'format OceanBase IPv6 JDBC URL');

assertEqual(normalizeJdbcHostFromUrl('[2001:db8::1002]'), '2001:db8::1002', 'normalize bracketed IPv6 host');
assertEqual(stripJdbcHostBrackets('[2001:db8::1002]'), '2001:db8::1002', 'strip bracketed IPv6 host');

console.log('jdbcUrl IPv6 helper tests passed');
