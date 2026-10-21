import { useEffect, useRef } from 'react';
import { useGlobalStore } from '@/store/global';
import { useZoerStore } from '@/store/zoer';

const useIframeMessage = () => {
  const lastUrlRef = useRef<string>('');
  const historyIndexRef = useRef<number>(0);
  const { setAppearance, setPrimaryColor } = useGlobalStore((state) => ({
    setAppearance: state.setAppearance,
    setPrimaryColor: state.setPrimaryColor,
  }));
  const { setZoerBoundInfo } = useZoerStore((state) => ({
    setZoerBoundInfo: state.setZoerBoundInfo,
  }));

  useEffect(() => {
    const handleLocationChange = () => {
      const currentUrl = window.location.href;
      if (currentUrl !== lastUrlRef.current) {
        lastUrlRef.current = currentUrl;

        // Update history index
        const currentIndex = window.history.state?.index ?? 0;
        window.parent.postMessage(
          {
            type: 'location_change',
            href: currentUrl,
            canGoBack: currentIndex > 0,
            canGoForward: currentIndex < historyIndexRef.current,
          },
          '*',
        );
        historyIndexRef.current = currentIndex;
      }
    };

    const handleMessage = (event: MessageEvent) => {
      if (event.data?.type === 'back') {
        window.history.back();
      } else if (event.data?.type === 'forward') {
        window.history.forward();
      } else if (event.data?.type === 'change_url') {
        window.location.href = event.data.href;
      } else if (event.data?.type === 'change_theme') {
        setAppearance(event.data.theme);
      } else if (event.data?.type === 'change_primary_color') {
        setPrimaryColor(event.data.primaryColor);
      } else if (event.data?.type === 'zoer_query_table') {
        setZoerBoundInfo(event.data.zoerBoundInfo);
      }
    };

    // Listen to the popstate event (the user clicks the browser's forward/back button)
    window.addEventListener('popstate', handleLocationChange);

    // Listen for hashchange events (the hash part of the URL changes)
    window.addEventListener('hashchange', handleLocationChange);

    // Listen for message events
    window.addEventListener('message', handleMessage);

    // Listen to pushState and replaceState events
    const originalPushState = window.history.pushState;
    const originalReplaceState = window.history.replaceState;

    window.history.pushState = function (data: any, unused: string, url?: string | URL | null) {
      const newState = {
        ...data,
        index: historyIndexRef.current + 1,
      };
      originalPushState.call(this, newState, unused, url);
      handleLocationChange();
    };

    window.history.replaceState = function (data: any, unused: string, url?: string | URL | null) {
      const newState = {
        ...data,
        index: historyIndexRef.current,
      };
      originalReplaceState.call(this, newState, unused, url);
      handleLocationChange();
    };

    // Send once initially
    handleLocationChange();

    // Send initialization message
    window.parent.postMessage(
      {
        type: 'chat2db_is_ready',
      },
      '*',
    );

    return () => {
      window.removeEventListener('popstate', handleLocationChange);
      window.removeEventListener('hashchange', handleLocationChange);
      window.removeEventListener('message', handleMessage);
      window.history.pushState = originalPushState;
      window.history.replaceState = originalReplaceState;
    };
  }, []);
};

export default useIframeMessage;
