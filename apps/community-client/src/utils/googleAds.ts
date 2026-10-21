import { runtimeEditionConfig } from '@/constants/runtimeEdition';
import { useGlobalStore } from '@/store/global';

/**
 * Google Ads conversion tracking for the overseas web edition.
 *
 * Registration and purchase happen at app.chat2db.ai, while the marketing site only brings in traffic.
 * Report sign_up and purchase conversions here, after the country is resolved and confirmed as non-CN.
 *
 * headScripts loads gtag.js asynchronously. The production configuration has no synchronous inline stub, so
 * ensureGtag installs the standard dataLayer-backed stub and queues commands until gtag.js is ready.
 *
 * Ad click (GCLID) -> marketing site or app -> successful registration/payment -> gtag conversion.
 * The linker carries GCLID from the marketing site to the app for cross-domain attribution.
 */

const GOOGLE_ADS_ID = 'AW-18121848527';
const SIGNUP_LABEL = 'A-SWCM-Wk7scEM_tlcFD';
const PURCHASE_LABEL = '3tdGCKfckrscEM_tlcFD';
const LINKER_DOMAINS = ['chat2db.ai', 'app.chat2db.ai'];

declare global {
  interface Window {
    dataLayer?: unknown[];
    gtag?: (...args: any[]) => void;
  }
}

/**
 * Ensure window.gtag and dataLayer are initialized, and return whether this is a browser environment.
 * The stub queues commands until the asynchronously loaded gtag.js library consumes them.
 */
const ensureGtag = (): boolean => {
  if (typeof window === 'undefined') {
    return false;
  }
  if (typeof window.gtag !== 'function') {
    window.dataLayer = window.dataLayer || [];
    // Keep function + arguments to match the official gtag snippet; an arrow function would change arguments.
    window.gtag = function gtag() {
      // eslint-disable-next-line prefer-rest-params
      window.dataLayer!.push(arguments);
    };
  }
  return true;
};

/**
 * Whether reporting to Google Ads is allowed.
 * 1. Browser environment with the gtag stub installed.
 * 2. Country resolved, avoiding a false overseas classification while isCN still defaults to false.
 * 3. Country is not CN.
 */
const adsEnabled = (): boolean => {
  if (!ensureGtag()) {
    return false;
  }
  const appConfig = useGlobalStore.getState().appConfig;
  return !!appConfig?.curCountry && !appConfig?.isCN;
};

/** Claim a localStorage key once; fail open if storage is unavailable. */
const claimOnce = (key: string): boolean => {
  try {
    if (window.localStorage.getItem(key)) {
      return false;
    }
    window.localStorage.setItem(key, String(Date.now()));
    return true;
  } catch {
    return true;
  }
};

let linkerInitialized = false;

/**
 * Initialize Ads configuration and the cross-domain linker once for overseas users.
 * Configure the linker before config so outbound links receive the _gl parameter.
 */
export const initGoogleAds = (): void => {
  if (linkerInitialized || !adsEnabled()) {
    return;
  }
  linkerInitialized = true;
  // Load the Ads tag directly as recommended. Mounting AW through GA4's secondary loader only supported
  // linker/remarketing and did not emit conversion requests, causing conversions to remain undetected.
  if (!document.querySelector(`script[src*="gtag/js?id=${GOOGLE_ADS_ID}"]`)) {
    const s = document.createElement('script');
    s.async = true;
    s.src = `https://www.googletagmanager.com/gtag/js?id=${GOOGLE_ADS_ID}`;
    document.head.appendChild(s);
  }
  window.gtag!('set', 'linker', { domains: LINKER_DOMAINS });
  window.gtag!('config', GOOGLE_ADS_ID);
};

/** Report registration once per orgId. */
export const trackSignUp = ({ orgId }: { orgId?: number | string }): void => {
  if (!adsEnabled() || orgId == null || orgId === '') {
    return;
  }
  if (!claimOnce(`${runtimeEditionConfig.googleAdsSignupOnceStorageKeyPrefix}_${orgId}`)) {
    return;
  }
  window.gtag!('event', 'conversion', {
    send_to: `${GOOGLE_ADS_ID}/${SIGNUP_LABEL}`,
  });
};

/** Report a purchase once per orderId; value must use the major currency unit. */
export const trackPurchase = ({
  orderId,
  value,
  currency = 'USD',
}: {
  orderId?: string;
  value?: number;
  currency?: string;
}): void => {
  if (!adsEnabled() || !orderId) {
    return;
  }
  if (!claimOnce(`${runtimeEditionConfig.googleAdsPurchaseOnceStorageKeyPrefix}_${orderId}`)) {
    return;
  }
  window.gtag!('event', 'conversion', {
    send_to: `${GOOGLE_ADS_ID}/${PURCHASE_LABEL}`,
    value,
    currency,
    transaction_id: orderId,
  });
};
