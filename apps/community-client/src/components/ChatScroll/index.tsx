import { IconButton } from '@chat2db/ui';
import React, {
  ForwardedRef,
  forwardRef,
  memo,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from 'react';
import { useStyles } from './style';
interface IProps extends React.HTMLAttributes<HTMLDivElement> {
  className?: string;
  beingOutput?: boolean;
  trigger: number;
}

interface ScrollToBottomOptions {
  smooth?: boolean;
  forcible?: boolean;
}

export interface ChatScrollRef {
  scrollToBottom: (options?: ScrollToBottomOptions) => void;
}

const SCROLL_THRESHOLD = {
  BOTTOM: 20, // Pixel threshold used to consider the view at the bottom.
  SHOW_BUTTON: 80, // Distance from the bottom at which to show the scroll button.
};

const ChatScroll = forwardRef((props: IProps, ref: ForwardedRef<ChatScrollRef>) => {
  const { className, children, beingOutput, trigger, ...restProps } = props;
  const { styles, cx } = useStyles();
  const containerRef = useRef<HTMLDivElement>(null);
  const [showScrollButton, setShowScrollButton] = useState(false);
  const isUserScrolling = useRef(false);

  const verifyScrollButton = useCallback(() => {
    if (containerRef.current) {
      const { scrollTop, scrollHeight, clientHeight } = containerRef.current;
      const computedStyle = window.getComputedStyle(containerRef.current);
      const isReversed = computedStyle.flexDirection === 'column-reverse';

      let distanceToBottom: number;

      if (isReversed) {
        // In reverse layout, scrollTop = 0 means the view is at the bottom.
        distanceToBottom = Math.abs(scrollTop);
      } else {
        // Normal layout.
        distanceToBottom = scrollHeight - scrollTop - clientHeight;
      }

      // Clear isUserScrolling when the scrollbar reaches the bottom.
      if (distanceToBottom <= SCROLL_THRESHOLD.BOTTOM) {
        isUserScrolling.current = false;
      }

      // Determine whether to show the scroll button.
      setShowScrollButton(distanceToBottom > SCROLL_THRESHOLD.SHOW_BUTTON);
    }
  }, []);

  const handleWheel = useCallback(() => {
    isUserScrolling.current = true;
  }, []);

  useEffect(() => {
    const container = containerRef.current;

    if (container) {
      container.addEventListener('scroll', verifyScrollButton);
      container.addEventListener('wheel', handleWheel);
      return () => {
        container.removeEventListener('scroll', verifyScrollButton);
        container.removeEventListener('wheel', handleWheel);
      };
    }
  }, []);

  useEffect(() => {
    scrollToBottom();
    // verifyScrollButton();
  }, [trigger]);

  const scrollToBottom = ({ smooth = false, forcible = false }: ScrollToBottomOptions = {}) => {
    if (forcible) {
      isUserScrolling.current = false;
    }
    if (!isUserScrolling.current) {
      // Ensure the DOM is updated before scrolling.
      setTimeout(() => {
        if (containerRef.current && forcible) {
          console.log('containerRef.current.scrollHeight-1', containerRef.current.scrollHeight);
          containerRef.current.scrollTo({
            top: containerRef.current.scrollHeight,
            behavior: smooth ? 'smooth' : 'instant',
          });
        }
      }, 0);
      setTimeout(() => {
        if (containerRef.current) {
          console.log('containerRef.current.scrollHeight-2', containerRef.current.scrollHeight);
          containerRef.current.scrollTo({
            top: containerRef.current.scrollHeight,
            behavior: smooth ? 'smooth' : 'instant',
          });
        }
      }, 0);
    }
  };

  useImperativeHandle(ref, () => ({
    scrollToBottom,
  }));

  return (
    <div className={cx(styles.container, className, 'bashful-scroller')} {...restProps} ref={containerRef}>
      {children}
      {showScrollButton && (
        <IconButton
          size={{
            boxSize: 30,
            iconSize: 18,
          }}
          className={cx(styles.scrollButton, { [styles.scrollButtonOutput]: beingOutput })}
          onClick={() => {
            scrollToBottom({
              smooth: true,
              forcible: true,
            });
          }}
          code="icon-scroll-bottom"
        />
      )}
    </div>
  );
});

export default memo(ChatScroll);
