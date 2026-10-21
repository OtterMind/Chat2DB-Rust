import i18n from '@/i18n';
import {
  LARGE_CELL_ERROR_CODE,
  LARGE_CELL_ERROR_MESSAGE,
  isLargeCellTokenExpiredError,
} from './largeCellValue';

const LARGE_CELL_MESSAGE_BY_CODE = {
  [LARGE_CELL_ERROR_CODE.FULL_VALUE_UNSUPPORTED]: LARGE_CELL_ERROR_MESSAGE.FULL_VALUE_UNSUPPORTED,
} as const;

export function getLargeCellDisplayMessage(message?: string | null) {
  if (!message) {
    return '';
  }
  const messageKey = LARGE_CELL_MESSAGE_BY_CODE[message as keyof typeof LARGE_CELL_MESSAGE_BY_CODE];
  return messageKey ? i18n(messageKey) : message;
}

export function getLargeCellErrorMessage(
  error: any,
  fallback: (typeof LARGE_CELL_ERROR_MESSAGE)[keyof typeof LARGE_CELL_ERROR_MESSAGE],
) {
  if (isLargeCellTokenExpiredError(error)) {
    return i18n(LARGE_CELL_ERROR_MESSAGE.TOKEN_EXPIRED);
  }
  return getLargeCellDisplayMessage(error?.errorMessage || error?.message) || i18n(fallback);
}
