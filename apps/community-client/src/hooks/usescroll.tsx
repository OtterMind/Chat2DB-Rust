import { useEffect, useRef } from 'react';

function useScrollToBottom() {
  const ref = useRef<HTMLElement | null>(null);

  useEffect(() => {
    let atBottom = true;

    const observer = new MutationObserver(() => {
      if (ref.current) {
        if (atBottom) {
          ref.current.scrollTop = ref.current.scrollHeight;
        }
        const { scrollTop, scrollHeight, clientHeight } = ref.current;
        atBottom = scrollTop + clientHeight === scrollHeight;
      }
    });

    if (ref.current) {
      observer.observe(ref.current, { childList: true, subtree: true });
    }

    return () => {
      observer.disconnect();
    };
  }, []);

  return ref;
}

export default useScrollToBottom;
