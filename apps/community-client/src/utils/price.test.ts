import { toMajorCurrencyUnit } from './price';

// Standalone assertion script runnable with `npx tsx src/utils/price.test.ts`.
function assertEqual(actual: any, expected: any, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${expected}, got ${actual}`);
  }
}

// toMajorCurrencyUnit converts minor units to major units for Google Ads values.
// Omitting /100 would inflate reported conversion values by 100 times.
const cases: Array<[number | string | undefined | null, number | undefined]> = [
  [12300, 123], // whole yuan
  [999, 9.99], // Including two decimal places
  [50, 0.5], // half a major currency unit
  [0, 0], // zero
  ['4900', 49], // string number
  [undefined, undefined], // Missing
  [null, undefined], // empty
  ['', undefined], // empty string
  ['abc', undefined], // Not a number
];

cases.forEach(([input, expected]) => {
  assertEqual(toMajorCurrencyUnit(input as any), expected, `toMajorCurrencyUnit(${String(input)})`);
});

// eslint-disable-next-line no-console
console.log(`price.test.ts: all ${cases.length} cases passed`);
