import React, { memo, useMemo, forwardRef, ForwardedRef, useImperativeHandle } from 'react';
import { useStyles } from './style';
import { useTableStyles } from '@/styles/table';
import { ConfigProvider, Spin, Table, type GetProp, type TableProps } from 'antd';
import { isEqualMemo } from '@/utils';
import { useSize } from 'ahooks';

type ColumnsType<
  T extends {
    onlyNumber?: boolean;
    editable?: boolean | undefined | ((rowData: any) => boolean | undefined);
    transitionValue?: (value: any, rowData: any) => string;
  },
> = GetProp<TableProps<T>, 'columns'>;

interface IProps {
  className?: string;
  // Table data.
  dataSource?: any[];
  // Table column configuration.
  columns: ColumnsType<any>;
  // Column width.
  sizes?: number[];
  // Whether to show tooltips.
  tooltip?: boolean;
  // Whether the first column is draggable.
  immutableFirstColumn?: boolean;
  // Whether columns are draggable.
  draggableColumn?: boolean;
  // Whether cells are editable.
  editable?: boolean;
  // Whether the table is loading.
  loading?: boolean;
  // Cell-edit callback.
  onChangeCell?: (index, columnName, value) => void;
  // Selected rows.
  selectedRows?: number[];
  // Selection-change callback.
  onSelectedRowsChange?: (selectedRows: number[]) => void;
  // Highlighted rows.
  highlightRows?: number[];
  // Highlighted columns.
  highlightColumns?: number[];
}

export interface BaseTableRef {
  scrollTo: (index: number) => void;
  scrollToTop: () => void;
  scrollToBottom: () => void;
  scrollToRight: () => void;
  scrollToLeft: () => void;
}

const NewBaseTable = forwardRef((props: IProps, ref: ForwardedRef<BaseTableRef>) => {
  const {
    className,
    dataSource,
    columns,
    loading,
    highlightRows,
    highlightColumns,
  } = props;
  const { styles, cx } = useStyles();
  const { styles: tableStyles } = useTableStyles();
  const supportBaseTableBoxRef = React.createRef<HTMLDivElement>();

  // Table column configuration.
  const convertedColumns: ColumnsType<any> = useMemo(() => {
    return columns.map((columnConfig: any, columnIndex) => {
      const isHighlightColumns = highlightColumns?.includes(columnIndex);

      const render = (value, rowData, rowIndex) => {
        let _value = value;
        if (columnConfig.transitionValue) {
          _value = columnConfig.transitionValue(value, rowData);
        }

        // Determine whether highlighting is needed.
        const isHighlightRows = highlightRows?.includes(rowIndex);

        return (
          <div
            className={cx(
              styles.plainText,
              { isHighlightRows: isHighlightRows },
              { isHighlightColumns: isHighlightColumns },
            )}
          >
            {_value}
          </div>
        );

        // return (
        //   <TableCell
        //     columnConfig={columnConfig}
        //     rowData={rowData}
        //     index={index}
        //     tooltip={tooltip}
        //     editable={editable}
        //     onChangeCell={onChangeCell}
        //     value={_value}
        //     // isSelected={isSelected}
        //     setSelectedRows={onSelectedRowsChange}
        //     styles={styles}
        //     cx={cx}
        //   />
        // );
      };

      return {
        ...columnConfig,
        title: (
          <div className={cx(styles.plainText, { isHighlightColumns: isHighlightColumns })}>{columnConfig.title}</div>
        ),
        render,
      };
    });
  }, [columns, highlightRows, highlightColumns]);

  const scrollTo = (index) => {
    supportBaseTableBoxRef.current?.scrollTo({
      top: index * 32,
      behavior: 'smooth',
    });
  };

  const scrollToTop = () => {
    supportBaseTableBoxRef.current?.scrollTo({
      top: 0,
      behavior: 'smooth',
    });
  };

  const scrollToBottom = () => {
    supportBaseTableBoxRef.current?.scrollTo({
      top: supportBaseTableBoxRef.current.scrollHeight,
      behavior: 'smooth',
    });
  };

  const scrollToRight = () => {
    const tables = supportBaseTableBoxRef.current?.querySelectorAll('table');
    const tableBody = supportBaseTableBoxRef.current?.querySelector('.art-table-body');
    const secondTable = tables?.[1];
    if (secondTable && tableBody) {
      tableBody.scrollLeft = secondTable.scrollWidth;
    }
  };

  const scrollToLeft = () => {
    const tables = supportBaseTableBoxRef.current?.querySelectorAll('table');
    const secondTable = tables?.[1];
    if (secondTable) {
      secondTable.scrollLeft = 0;
    }
  };

  useImperativeHandle(ref, () => ({
    scrollTo,
    scrollToTop,
    scrollToBottom,
    scrollToRight,
    scrollToLeft,
  }));

  const supportBaseTableBoxSize = useSize(supportBaseTableBoxRef);

  return (
    <div className={cx(styles.supportBaseTableBox, className)} ref={supportBaseTableBoxRef}>
      {loading ? (
        <div className={styles.spinBox}>
          <Spin />
        </div>
      ) : (
        supportBaseTableBoxSize && (
          <ConfigProvider
            theme={{
              components: {
                Table: {
                  headerBorderRadius: 0,
                },
              },
            }}
          >
            <Table
              className={tableStyles.baseTable}
              virtual
              bordered
              dataSource={dataSource}
              columns={convertedColumns}
              pagination={false}
              scroll={{
                x: supportBaseTableBoxSize.width - 1, // 1 is the border width.
                y: supportBaseTableBoxSize.height - 32, // 32 is the header height.
              }}
            />
          </ConfigProvider>
        )
      )}
    </div>
  );
});

export default memo(NewBaseTable, (prevProps, nextProps) => {
  return isEqualMemo(
    [prevProps.dataSource, nextProps.dataSource],
    [prevProps.highlightRows, nextProps.highlightRows],
    [prevProps.highlightColumns, nextProps.highlightColumns],
    [prevProps.columns, nextProps.columns],
    [prevProps.selectedRows, nextProps.selectedRows],
  );
});
