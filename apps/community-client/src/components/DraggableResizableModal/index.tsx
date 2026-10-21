import React, { useState, useRef, useEffect, memo, forwardRef, useImperativeHandle, ForwardedRef } from 'react';
import ReactDOM from 'react-dom';
import Draggable from 'react-draggable';
import { ResizableBox } from 'react-resizable';
import 'react-resizable/css/styles.css';
import { useStyles } from './style';

interface SizeConstraints {
  minWidth?: number;
  minHeight?: number;
  maxWidth?: number;
  maxHeight?: number;
}

interface InitialDimensions {
  width: number;
  height?: number;
  x: number;
  y: number;
}

// n: north (top)
// s: south (bottom)
// e: east (right)
// w: west (left)
// ne: northeast (top-right)
// nw: northwest (top-left)
// se: southeast (bottom-right)
// sw: southwest (bottom-left)

type ResizeHandle = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw';

interface DraggableResizableModalProps {
  className?: string;
  sizeConstraints?: SizeConstraints;
  initialDimensions: InitialDimensions;
  resizeHandles?: ResizeHandle[];
  children: React.ReactNode;
  onClose?: () => void;
}

export interface DraggableResizableModalRef {
  open: (position) => void;
  close: () => void;
  changeDimensions: (dimensions: Partial<InitialDimensions>) => void;
}

const DraggableResizableModal = forwardRef(
  (props: DraggableResizableModalProps, ref: ForwardedRef<DraggableResizableModalRef>) => {
    const { sizeConstraints, initialDimensions, resizeHandles, children, className, onClose } = props;
    const [visible, setVisible] = useState(false);
    const modalRef = useRef<HTMLDivElement | null>(null);
    const { styles, cx } = useStyles();
    const [dimensions, setDimensions] = useState(initialDimensions);
    // Get the browser dimensions.
    const browserWidth = window.innerWidth;
    const browserHeight = window.innerHeight;

    useEffect(() => {
      const handleClickOutside = (event: MouseEvent) => {
        // Add a check to see if the event target is a resize handle
        if (
          modalRef.current &&
          !modalRef.current.contains(event.target as Node) &&
          !(event.target as HTMLElement).classList.contains('react-resizable-handle')
        ) {
          setVisible(false);
        }
      };

      if (visible) {
        document.addEventListener('mousedown', handleClickOutside);
      } else {
        onClose?.();
        document.removeEventListener('mousedown', handleClickOutside);
      }

      return () => {
        document.removeEventListener('mousedown', handleClickOutside);
      };
    }, [visible]);

    const handleResize = (
      event: React.SyntheticEvent,
      { size, handle }: { size: { width: number; height: number }; handle: ResizeHandle },
    ) => {
      setDimensions((prev) => {
        let newX = prev.x;
        let newY = prev.y;

        // Adjust the position for the drag direction.
        if (handle.includes('w')) {
          newX -= size.width - prev.width;
        }
        if (handle.includes('n')) {
          newY -= size.height - (prev.height || 0);
        }

        return {
          width: size.width,
          height: size.height,
          x: newX,
          y: newY,
        };
      });
    };

    useEffect(() => {
      // Close the dialog with Escape.
      const handleKeyDown = (event: KeyboardEvent) => {
        if (event.key === 'Escape') {
          setVisible(false);
        }
      };
      document.addEventListener('keydown', handleKeyDown);
      return () => {
        document.removeEventListener('keydown', handleKeyDown);
      };
    }, []);

    useImperativeHandle(ref, () => ({
      open: (position) => {
        setDimensions({
          ...dimensions,
          ...(position || {}),
        });
        setVisible(true);
      },
      close: () => {
        setVisible(false);
      },
      changeDimensions: (_dimensions) => {
        setDimensions({
          ...dimensions,
          ..._dimensions,
        });
      },
    }));

    if (!visible) {
      return null;
    }

    const modalContent = (
      <div className={styles.modalOverlay}>
        <Draggable
          handle=".dragHandle"
          bounds="body"
          defaultPosition={{ x: initialDimensions.x, y: initialDimensions.y }}
          onStop={(e, data) => {
            setDimensions((prev) => ({
              ...prev,
              x: data.x,
              y: data.y,
            }));
          }}
          position={{ x: dimensions.x, y: dimensions.y }}
        >
          <ResizableBox
            width={dimensions.width}
            height={dimensions.height || 'auto'}
            minConstraints={[sizeConstraints?.minWidth || 0, sizeConstraints?.minHeight || 0]}
            maxConstraints={[sizeConstraints?.maxWidth || browserWidth, sizeConstraints?.maxHeight || browserHeight]}
            onResize={handleResize}
            className="modal-container"
            resizeHandles={resizeHandles || ['n', 'e', 's', 'w', 'ne', 'nw', 'se', 'sw']}
          >
            <div
              ref={modalRef}
              className={cx('dragHandle', styles.modalContent, className)}
              style={{ width: '100%', height: '100%' }}
            >
              {children}
            </div>
          </ResizableBox>
        </Draggable>
      </div>
    );

    return ReactDOM.createPortal(modalContent, document.body);
  },
);

export default memo(DraggableResizableModal);
