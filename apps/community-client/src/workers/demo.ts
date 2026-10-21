// fakeWorker.js

self.onmessage = function (event) {
  const { type, data } = event.data;

  switch (type) {
    case 'generateLargeArray':
      handleGenerateLargeArray();
      break;

    case 'incrementArray':
      handleIncrementArray(data);
      break;

    default:
      console.error('Unknown message type:', type);
  }
};

// Handling generateLargeArray messages
function handleGenerateLargeArray() {
  const largeArray = generateLargeArray();
  self.postMessage({ type: 'generateLargeArray', data: largeArray });
}

// Handling incrementArray messages
function handleIncrementArray(arr) {
  const incrementedArray = incrementArray(arr);
  self.postMessage({ type: 'incrementArray', data: incrementedArray });
}

// Method 1: Generate an array with a length of 100,000, each item is initially 0, and then set it as a subscript
export function generateLargeArray() {
  console.time('generateLargeArray');
  const arr = new Array(1000000).fill(0); // An array of length 100,000, each item is 0
  for (let i = 0; i < arr.length; i++) {
    arr[i] = i; // Set each item to its index
  }
  console.timeEnd('generateLargeArray');
  return arr;
}

// Method 2: Accept an array and add 1 to each item
export function incrementArray(arr) {
  for (let i = 0; i < arr.length; i++) {
    arr[i] += 1;
  }
  return arr;
}
