import { memo, useEffect, useRef, useState } from 'react';
import { useStyles } from './style';
import { Table, type TableProps } from 'antd';

interface IProps extends TableProps {
  className?: string;
  subHeight?: number;
}

export default memo<IProps>((props) => {
  const { className, subHeight, ...tableProps } = props;
  const { styles, cx } = useStyles();
  const tableBoxRef = useRef<any>(null);
  const [tableScrollY, setTableScrollY] = useState(0);

  // Track tableBoxRef height changes and update tableScrollY when resized.
  useEffect(() => {
    const resizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { height } = entry.contentRect;
        let fHeight = height - (subHeight || 55);
        if (tableProps.pagination) {
          fHeight = fHeight - 50;
        }
        setTableScrollY(fHeight);
      }
    });

    if (tableBoxRef.current) {
      resizeObserver.observe(tableBoxRef.current);
    }

    return () => {
      resizeObserver.disconnect();
    };
  }, []);

  return (
    <div className={cx(className, styles.tableBox)} ref={tableBoxRef}>
      <Table
        scroll={{ y: tableScrollY }}
        style={{
          height: '100%',
        }}
        sticky
        pagination={false}
        {...tableProps}
      />
    </div>
  );
});
