const OSCAR_ICON_SYMBOL = `<svg xmlns="http://www.w3.org/2000/svg"><symbol id="icon-colourful-OSCAR" viewBox="0 0 1024 1024"><defs><linearGradient id="oscar-left" x1="280" x2="520" y1="780" y2="90" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#d8e7ff"></stop><stop offset="0.42" stop-color="#2d66bd"></stop><stop offset="1" stop-color="#003f9f"></stop></linearGradient><linearGradient id="oscar-right" x1="520" x2="760" y1="90" y2="780" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#e9f2ff"></stop><stop offset="0.45" stop-color="#6688c9"></stop><stop offset="1" stop-color="#0048b6"></stop></linearGradient><linearGradient id="oscar-inner" x1="382" x2="524" y1="782" y2="405" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#004bb5"></stop><stop offset="0.58" stop-color="#c8dcff"></stop><stop offset="1" stop-color="#eef5ff"></stop></linearGradient><linearGradient id="oscar-inner-right" x1="516" x2="662" y1="404" y2="780" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#0057c6"></stop><stop offset="1" stop-color="#0a3f9d"></stop></linearGradient></defs><circle cx="512" cy="512" r="470" fill="#fff" stroke="#0a52b5" stroke-width="18"></circle><circle cx="512" cy="512" r="431" fill="none" stroke="#0a52b5" stroke-width="10"></circle><circle cx="512" cy="512" r="406" fill="none" stroke="#0a52b5" stroke-width="8"></circle><circle cx="512" cy="512" r="365" fill="none" stroke="#0a52b5" stroke-width="8"></circle><circle cx="512" cy="512" r="313" fill="none" stroke="#0a52b5" stroke-width="7"></circle><path d="M512 82 250 780h116l146-377z" fill="url(#oscar-left)"></path><path d="M512 82 774 780H658L512 403z" fill="url(#oscar-right)"></path><path d="M512 403 366 780l146-141z" fill="url(#oscar-inner)"></path><path d="M512 403 658 780 512 639z" fill="url(#oscar-inner-right)"></path><text x="512" y="848" fill="#0a4cb3" font-family="Arial, Helvetica, sans-serif" font-size="116" font-weight="700" letter-spacing="8" text-anchor="middle">CASC</text></symbol></svg>`;

const injectOscarIcon = () => {
  if (typeof document === 'undefined') {
    return;
  }

  document.getElementById('icon-colourful-OSCAR')?.remove();

  const wrapper = document.createElement('div');
  wrapper.innerHTML = OSCAR_ICON_SYMBOL;
  const svg = wrapper.getElementsByTagName('svg')[0];

  if (!svg || !document.body) {
    return;
  }

  svg.setAttribute('aria-hidden', 'true');
  svg.style.position = 'absolute';
  svg.style.width = '0';
  svg.style.height = '0';
  svg.style.overflow = 'hidden';
  document.body.insertBefore(svg, document.body.firstChild);
};

if (typeof document !== 'undefined') {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', injectOscarIcon, { once: true });
  } else {
    injectOscarIcon();
  }
}

export {};
