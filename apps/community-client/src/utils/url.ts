/**
 * Get url parameters
 * @param paramName
 * @returns
 */

import { isDesktop } from './env';
import jcefApi from '@/jcef';

export function getUrlParam(paramName) {
  // Get current URL
  const currentUrl = window.location.href;

  // Get the query string in the URL
  const queryString = currentUrl.split('?')[1];

  // If there is no query string, return directly
  if (queryString === undefined) {
    return;
  }

  // Split query string into array
  const paramList = queryString.split('&');

  // Iterate through each parameter
  for (let i = 0; i < paramList.length; i++) {
    // Check if the name of each parameter matches
    const param = paramList[i].split('=')[0];
    if (param === paramName) {
      // Return parameter value
      return paramList[i].split('=')[1];
    }
  }

  // If the parameter is not found, returns null
  return null;
}

export function getAllUrlParams(url?: string) {
  // If no url is provided, the url of the current page is used
  const queryString = url ? url.split('?')[1] : window.location.search.slice(1);

  const params: any = {};

  // Using URLSearchParams to parse query parameters
  const query = new URLSearchParams(queryString);

  // Iterate through all parameters
  query.forEach((value, key) => {
    // If the key already exists, add the value to the array
    if (Object.prototype.hasOwnProperty.call(params, key)) {
      if (!Array.isArray(params[key])) {
        params[key] = [params[key]]; // Convert single value to array
      }
      params[key].push(value);
    } else {
      // If the key does not exist, add the value directly
      params[key] = value;
    }
  });

  return params;
}

export function updateUrl(url, pathname, params) {
  // Create a URL object
  const _url = new URL(url);

  if (pathname) {
    _url.pathname = pathname;
  }

  // Iterate over parameter objects
  Object.keys(params || {}).forEach((key) => {
    // If the parameter value is an array, iterate over the array
    if (Array.isArray(params[key])) {
      params[key].forEach((val) => {
        _url.searchParams.append(key, val);
      });
    } else {
      _url.searchParams.append(key, params[key]);
    }
  });
  return _url.toString();
}

/**
 * Update URL parameters
 * @param key
 * @param value
 * @returns
 */
export function updateQueryStringParameter(key, value) {
  const uri = window.location.href;
  if (!value) {
    return uri;
  }
  const re = new RegExp('([?&])' + key + '=.*?(&|$)', 'i');
  const separator = uri.indexOf('?') !== -1 ? '&' : '?';
  if (uri.match(re)) {
    return uri.replace(re, '$1' + key + '=' + value + '$2');
  } else {
    return uri + separator + key + '=' + value;
  }
}

/**
 * Formatting parameters
 * @param obj
 * @returns
 */
export function formatParams(obj: { [key: string]: any }) {
  const params = new URLSearchParams();
  Object.entries(obj).forEach(([key, value]) => {
    if (value === undefined || value === null) {
      return;
    }
    if (Array.isArray(value)) {
      value.forEach((item) => {
        params.append(key, item);
      });
    } else {
      params.append(key, value);
    }
  });
  return params.toString();
}

/**
 * Generate url
 * @param key
 * @returns
 */

export function generateUrl(key: string) {
  if (isDesktop) {
    return window.location.href.split('/#/')[0] + '/#/' + key;
  }
  return window.location.origin + '/' + key;
}

/**
 * Determine whether the current page is a share page
 * @param key
 * @returns
 */

export function checkIsSharePage() {
  return window.location.pathname.includes('/share');
}

export function openWebPage(url: string, target: '_blank' | '_self' = '_blank') {
  if (isDesktop) {
    jcefApi.openWebPage(url);
  } else {
    window.open(url, target);
  }
}
