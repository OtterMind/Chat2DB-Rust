import { memo, useMemo } from 'react';
import { useStyles } from './style';
import CanvasTable from '@/blocks/CanvasTable';
import { INormalizedData } from '@/blocks/BI/Chart/typings';
import { ITableHeaderItem } from '@/typings';
import { useGlobalStore } from '@/store/global';

interface IProps {
  className?: string;
  data?: INormalizedData;
  headerList: ITableHeaderItem[];
}

export default memo<IProps>((props) => {
  const { className, data: tableData, headerList } = props;
  const { styles, cx } = useStyles();
  const customFontSize = useGlobalStore.getState().baseSetting.customFontSize;
  const columns = useMemo(() => {
    if (typeof tableData !== 'object' || !tableData?.length) {
      return headerList.map((k) => ({
        title: k.name,
        field: k.name,
        width: 'auto',
        fontSize: customFontSize,
      }));
    }
    return Object.keys(tableData[0]).map((key) => {
      return {
        title: key,
        field: key,
        width: 'auto',
        fontSize: customFontSize,
      };
    });
  }, [tableData, headerList]);

  return (
    <div className={cx(styles.chartTable, className)}>
      {!!columns?.length && (
        <CanvasTable
          records={tableData || []}
          columns={columns}
          options={{
            widthMode: 'autoWidth',
            frozenColCount: 0, // Number of frozen columns
          }}
        />
      )}
    </div>
  );
});
