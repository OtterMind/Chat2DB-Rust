import React, { memo, useEffect, useMemo, useRef, useState } from 'react';
import GridLayout, { ReactGridLayoutProps } from 'react-grid-layout';
import { createGridPattern } from '@/utils/file';
import { useStyles } from './style';
import 'react-grid-layout/css/styles.css';
import 'react-resizable/css/styles.css';

interface IProps extends ReactGridLayoutProps {
  className?: string;
  children?: React.ReactNode;
  layout?: any[];
  draggableHandle?: string;
  isResizable?: boolean;
  isDraggable?: boolean;
  onLayoutChange?: (layout: any) => void;
}

export default memo<IProps>((props) => {
  const { className, children, onLayoutChange, ...rest } = props;
  const [gridBackground, setGridBackground] = useState({
    background: '',
    width: 0,
  });
  const { styles, cx, theme } = useStyles({ gridBackground: gridBackground.background });
  const gridLayoutRef = useRef<HTMLDivElement>(null);
  const [gridLayoutWidth, setGridLayoutWidth] = useState(0);
  const [isDragging, setIsDragging] = useState(false);
  const [isResizing, setIsResizing] = useState(false);

  const showDargBackground = useMemo(() => {
    return isDragging || isResizing;
  }, [isDragging, isResizing]);

  const canRender = useMemo(() => {
    return gridLayoutWidth > 0;
  }, [gridLayoutWidth]);

  useEffect(() => {
    const nextGridBackground = createGridPattern(gridLayoutWidth / 12 - 8, 90, 8, 8, theme.colorBorder);
    setGridBackground({
      background: `url(${nextGridBackground})`,
      width: gridLayoutWidth,
    });
  }, [showDargBackground]);

  useEffect(() => {
    // Track gridLayoutRef width changes.
    const gridLayoutObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setGridLayoutWidth(entry.contentRect.width);
      }
    });
    gridLayoutObserver.observe(gridLayoutRef.current as Element);

    return () => {
      gridLayoutObserver.disconnect();
    };
  }, [theme]);

  return (
    <div ref={gridLayoutRef} className={cx(styles.gridLayoutBox, { [styles.showDargBackground]: showDargBackground })}>
      {canRender && (
        <GridLayout
          onDragStart={() => setIsDragging(true)}
          onDragStop={(layout) => {
            onLayoutChange?.(layout);
            setIsDragging(false);
          }}
          onResizeStart={() => setIsResizing(true)}
          onResizeStop={(layout) => {
            onLayoutChange?.(layout);
            setIsResizing(false);
          }}
          cols={12}
          rowHeight={90}
          margin={[8, 8]}
          width={gridLayoutWidth}
          {...rest}
        >
          {children}
        </GridLayout>
      )}
    </div>
  );
});
