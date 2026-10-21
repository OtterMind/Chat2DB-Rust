import { useEffect } from 'react';
import { copyToClipboard } from '@/utils';
import { setFocusedContent, getFocusedContent } from '@/store/common/copyFocusedContent';

// If the user clicks on an element that is not copyable, clear the selected content
function useCopyFocusData() {
  // Register shortcut key to monitor cmd+c or ctrl+c to copy focusedContent
  useEffect(() => {
    const handleCopy = (e: KeyboardEvent) => {
      const focusedContent = getFocusedContent();
      if (e.code === 'KeyC' && (e.metaKey || e.ctrlKey)) {
        if (!focusedContent) return;
        copyToClipboard(focusedContent);
        // Prevent the browser copy event from running after copyToClipboard.
        // And it is necessary to load copy to prevent normal copy behavior from being blocked.
        e.preventDefault();
      }
    };
    document.addEventListener('keydown', handleCopy);
    return () => {
      document.removeEventListener('keydown', handleCopy);
    };
  }, []);

  useEffect(() => {
    const handleClick = (event) => {
      const targetElement = event.target as Element;
      if (!targetElement.closest('[data-chat2db-general-can-copy-element]')) {
        setFocusedContent(null);
      }
    };
    document.addEventListener('click', handleClick);
    document.addEventListener('contextmenu', handleClick);
    return () => {
      document.removeEventListener('click', handleClick);
      document.removeEventListener('contextmenu', handleClick);
    };
  }, []);
}

export default useCopyFocusData;
