export const isEmbedIframePage = () => {
  if (typeof window === 'undefined') {
    return false;
  }

  if (new URLSearchParams(window.location.search).get('iframe')) {
    return true;
  }

  try {
    return window.self !== window.top;
  } catch {
    return true;
  }
};
