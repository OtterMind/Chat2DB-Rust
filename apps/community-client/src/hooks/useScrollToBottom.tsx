import { useUpdateEffect } from 'ahooks';
import React from 'react';

const useScrollToBottom = (
  bottomRef: React.RefObject<HTMLElement>,
  triggerVariable: any,
  behavior?: ScrollBehavior,
) => {
  useUpdateEffect(() => {
    setTimeout(() => {
      scrollToBottom();
    }, 0);
  }, [triggerVariable]);

  const scrollToBottom = () => {
    bottomRef.current?.scrollIntoView({
      behavior: behavior || 'auto',
    });
  };

  return { scrollToBottom };
};

export default useScrollToBottom;
