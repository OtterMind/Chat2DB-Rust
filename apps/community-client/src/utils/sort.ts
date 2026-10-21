/**
 * Compare the size of two string numbers, including large numbers
 * @param a
 * @param b
 * @returns
 */
export function compareStrings(a: string, b: string) {
  if (!a) {
    return -1;
  }

  if (!b) {
    return 1;
  }

  // Compare string lengths
  if (a.length !== b.length) {
    return a.length - b.length;
  }

  // Compare character ASCII code values bit by bit
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) {
      return a.charCodeAt(i) - b.charCodeAt(i);
    }
  }

  // If the two strings are exactly equal, return 0
  return 0;
}

/**
 * Sort data
 * @param {Array} data - the data to be sorted
 * @param {string} field - field name
 * @param {string} order - sorting method ('asc' or 'desc')
 * @return {Array} - sorted data
 */
export function sortData(data, field, order) {
  const dataCopy = [...data];
  return dataCopy.sort((a, b) => {
    const fieldA = a[field];
    const fieldB = b[field];

    // Handling null and undefined cases
    if (fieldA === null || fieldA === undefined) return order === 'asc' ? -1 : 1;
    if (fieldB === null || fieldB === undefined) return order === 'asc' ? 1 : -1;

    // If the field value is a number
    if (!isNaN(fieldA) && !isNaN(fieldB)) {
      return order === 'asc' ? fieldA - fieldB : fieldB - fieldA;
    }

    // If the field value is a string
    if (typeof fieldA === 'string' && typeof fieldB === 'string') {
      return order === 'asc' ? fieldA.localeCompare(fieldB) : fieldB.localeCompare(fieldA);
    }

    // In other cases (such as different types of data), keep the original order
    return 0;
  });
}
