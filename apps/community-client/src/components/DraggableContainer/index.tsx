import { memo, useEffect, useRef } from 'react';
import styles from './index.less';
import classnames from 'classnames';

interface IProps {
  className?: string;
  children: any; // TODO: Type the accepted array in TypeScript.
  min?: number;
  layout?: 'row' | 'column';
  onResize?: (data: number) => void;
  showLine?: boolean;
}

export default memo<IProps>((props: IProps) => {
  const { children, showLine = true, onResize, min, className, layout = 'row' } = props;
  const volatileRef = children[0]?.ref || children[1]?.ref;

  const dividerRef = useRef<HTMLDivElement | null>(null);
  const dividerLine = useRef<HTMLDivElement | null>(null);

  const isRow = layout === 'row';

  useEffect(() => {
    if (!dividerRef.current) {
      return;
    }

    dividerRef.current.onmousedown = (e) => {
      if (!volatileRef?.current) return;
      e.preventDefault();
      const clientStart = isRow ? e.clientX : e.clientY;
      const volatileBoxXY = isRow ? volatileRef.current.offsetWidth : volatileRef.current.offsetHeight;
      document.onmousemove = (_e) => {
        moveHandle(isRow ? _e.clientX : _e.clientY, volatileRef.current, clientStart, volatileBoxXY);
      };
      document.onmouseup = () => {
        document.onmouseup = null;
        document.onmousemove = null;
      };
    };
  }, []);

  const moveHandle = (nowClientXY: any, leftDom: any, clientStart: any, volatileBoxXY: any) => {
    const computedXY = nowClientXY - clientStart;
    let finalXY = 0;

    // Use + when the first child is resizable and - when the second child is resizable.
    finalXY = children[0]?.ref ? volatileBoxXY + computedXY : volatileBoxXY - computedXY;

    if (min && finalXY < min) {
      return;
    }
    if (isRow) {
      leftDom.style.width = finalXY + 'px';
    } else {
      leftDom.style.height = finalXY + 'px';
    }
    onResize && onResize(finalXY);
  };

  return (
    <div className={classnames(styles.box, { [styles.boxColumn]: !isRow }, className)}>
      {children[0]}
      {
        <div
          style={{ display: showLine ? 'block' : 'none' }}
          ref={dividerLine}
          className={classnames(styles.divider, { [styles.displayDivider]: !children[1] })}
        >
          <div ref={dividerRef} className={styles.dividerCenter} />
        </div>
      }
      {children[1]}
    </div>
  );
});
