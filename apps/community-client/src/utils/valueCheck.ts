// Determine whether a value is null/undefined
export const isNullOrUndefined = (value: any) => {
  return value === null || value === undefined;
};

// If a value is null convert to string<null>
export const formatDbNullValue = (value: string | null, defaultValue: string = '') => {
  return value === null ? defaultValue : value;
};
