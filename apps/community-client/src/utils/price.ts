export const formatPrice = (price?: string | number): string => {
  if (price === undefined || price === null || isNaN(Number(price))) {
    return '0';
  }

  // Divide the price by 100
  const priceInYuan = Number(price) / 100;

  // Convert to string to handle decimal part
  let formattedPrice = priceInYuan.toString();

  // Check decimal part
  if (formattedPrice.includes('.')) {
    const [integerPart, decimalPart] = formattedPrice.split('.');

    if (decimalPart.length > 2) {
      // If the length of the decimal part exceeds 2, keep two decimal places
      formattedPrice = priceInYuan.toFixed(2);
    } else if (decimalPart.length === 1) {
      // If the length of the decimal part is 1, keep one decimal place
      formattedPrice = priceInYuan.toFixed(1);
    } else {
      // If the length of the decimal part is 2 but the end is "00", then remove the decimal part
      formattedPrice = integerPart;
    }
  }

  return formattedPrice;
};

export const formatCurrency = (currency?: string) => {
  const currencySymbol = {
    USD: '$',
    CNY: '¥',
    EU: '€',
  };
  if (currency === undefined || currency === null) {
    return currencySymbol['USD'];
  }
  return currencySymbol[currency] || currencySymbol['USD'];
};

export const formatPriceWithCurrency = (currency: string, price: string) => {
  return `${formatCurrency(currency)}${formatPrice(price)}`;
};

export const oldFormatPrice = (currency: string, price: string) => {
  const currencySymbol = {
    USD: '$',
    CNY: '¥',
    EU: '€',
  };
  return `${currencySymbol[currency] || currencySymbol['USD']}${(Number(price) / 100).toFixed(2)}`;
};

/**
 * Convert a minor currency amount to its major unit for the Google Ads conversion value.
 * Divide by 100 like formatPrice, but return a number without display truncation; invalid input returns undefined.
 */
export const toMajorCurrencyUnit = (minor?: number | string): number | undefined => {
  if (minor === undefined || minor === null || minor === '' || isNaN(Number(minor))) {
    return undefined;
  }
  return Number(minor) / 100;
};
