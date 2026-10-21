import type { ICharset, ICollation } from '@/typings';

export interface CreateDatabaseOption {
  label: string;
  value: string;
}

export const buildCharsetOptions = (charsets: ICharset[]): CreateDatabaseOption[] =>
  charsets.map((item) => ({ label: item.charsetName, value: item.charsetName }));

export const buildCollationOptions = (
  charsets: ICharset[],
  collations: ICollation[],
  selectedCharset?: string,
): CreateDatabaseOption[] => {
  const normalizedCharset = selectedCharset?.toLowerCase();
  const defaultCollation = charsets
    .find((item) => item.charsetName.toLowerCase() === normalizedCharset)
    ?.defaultCollationName?.toLowerCase();

  return collations
    .filter((item) => {
      const collationName = item.collationName.toLowerCase();
      return (
        !normalizedCharset || collationName === defaultCollation || collationName.startsWith(`${normalizedCharset}_`)
      );
    })
    .map((item) => ({ label: item.collationName, value: item.collationName }));
};
