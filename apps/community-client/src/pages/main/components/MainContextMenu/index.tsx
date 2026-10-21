import { ReactNode, useEffect, useLayoutEffect, useRef, useState } from 'react';
import ReactDOM from 'react-dom';

import { useStyles } from './style';

const VIEWPORT_PADDING = 8;

interface MainContextMenuProps {
  x: number;
  y: number;
  children: ReactNode;
  className?: string;
  onClose: () => void;
}

const MainContextMenu = ({ x, y, children, className, onClose }: MainContextMenuProps) => {
  const { styles } = useStyles();
  const popupRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x, y });

  useLayoutEffect(() => {
    setPosition({ x, y });
  }, [x, y]);

  useLayoutEffect(() => {
    if (!popupRef.current) {
      return;
    }

    const rect = popupRef.current.getBoundingClientRect();
    const nextX = Math.max(VIEWPORT_PADDING, Math.min(position.x, window.innerWidth - rect.width - VIEWPORT_PADDING));
    const nextY = Math.max(VIEWPORT_PADDING, Math.min(position.y, window.innerHeight - rect.height - VIEWPORT_PADDING));

    if (nextX !== position.x || nextY !== position.y) {
      setPosition({ x: nextX, y: nextY });
    }
  }, [position.x, position.y]);

  useEffect(() => {
    const handlePointerDown = (event: MouseEvent) => {
      if (popupRef.current?.contains(event.target as Node)) {
        return;
      }
      onClose();
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        onClose();
      }
    };

    document.addEventListener('mousedown', handlePointerDown, true);
    document.addEventListener('keydown', handleKeyDown, true);
    window.addEventListener('resize', onClose);
    window.addEventListener('blur', onClose);
    window.addEventListener('scroll', onClose, true);
    return () => {
      document.removeEventListener('mousedown', handlePointerDown, true);
      document.removeEventListener('keydown', handleKeyDown, true);
      window.removeEventListener('resize', onClose);
      window.removeEventListener('blur', onClose);
      window.removeEventListener('scroll', onClose, true);
    };
  }, [onClose]);

  return ReactDOM.createPortal(
    <div
      ref={popupRef}
      className={[styles.popup, className].filter(Boolean).join(' ')}
      style={{ left: position.x, top: position.y }}
      onClick={(event) => event.stopPropagation()}
      onMouseDown={(event) => event.stopPropagation()}
      onContextMenu={(event) => {
        event.preventDefault();
        event.stopPropagation();
      }}
    >
      {children}
    </div>,
    document.body,
  );
};

export default MainContextMenu;
