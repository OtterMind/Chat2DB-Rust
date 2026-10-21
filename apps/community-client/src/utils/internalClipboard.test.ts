import {
  clearInternalClipboard,
  getInternalResultGridClipboard,
  setInternalResultGridClipboard,
} from './internalClipboard';

function assertEqual(actual: any, expected: any, message: string) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

clearInternalClipboard();
setInternalResultGridClipboard([
  ['1', '2', '5'],
  ['2', '1', '5'],
]);

assertEqual(
  getInternalResultGridClipboard('1\t2\t5\n2\t1\t5'),
  [
    ['1', '2', '5'],
    ['2', '1', '5'],
  ],
  'matching clipboard text returns the internal result-grid matrix',
);
assertEqual(
  getInternalResultGridClipboard('1\t2\t5'),
  null,
  'different clipboard text does not reuse stale result-grid metadata',
);

const firstRead = getInternalResultGridClipboard('1\t2\t5\n2\t1\t5');
if (firstRead) {
  firstRead[0][0] = 'changed';
}
assertEqual(
  getInternalResultGridClipboard('1\t2\t5\n2\t1\t5')?.[0][0],
  '1',
  'callers cannot mutate the stored result-grid matrix',
);

clearInternalClipboard();
assertEqual(
  getInternalResultGridClipboard('1\t2\t5\n2\t1\t5'),
  null,
  'clearing the internal clipboard removes the result-grid matrix',
);

console.log('internal clipboard tests passed');
