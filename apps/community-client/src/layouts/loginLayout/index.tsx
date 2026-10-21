import { runtimeEditionConfig } from '@/constants/runtimeEdition';
import { useShortcutManager } from '@/utils/shortcutManager';
import { useEffect } from 'react';
import { Outlet } from 'umi';
import useInitQuery from '../init/initQuery';

function loginLayout() {
  // Register shortcut keys
  useShortcutManager();

  useEffect(() => {
    if (!runtimeEditionConfig.commercialAccount) {
      return;
    }

    const redirectStorageKey = runtimeEditionConfig.loginRedirectStorageKey;
    const storedRedirect = localStorage.getItem(redirectStorageKey);
    if (storedRedirect) {
      try {
        const { url, timestamp } = JSON.parse(storedRedirect);
        const currentTime = new Date().getTime();
        if (currentTime - timestamp < 30000) {
          // 30 seconds
          localStorage.removeItem(redirectStorageKey);
          window.location.href = decodeURIComponent(url);
        } else {
          localStorage.removeItem(redirectStorageKey);
        }
      } catch (error) {
        console.error('Error parsing login redirect url:', error);
        localStorage.removeItem(redirectStorageKey);
      }
    }
  }, []);

  // serviceStatus initialization data
  const { initQueryLoaded } = useInitQuery();
  return initQueryLoaded && <Outlet />;
}

export default loginLayout;
