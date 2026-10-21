import { memo, useMemo } from 'react';
import { useStyles } from './style';
import EditableCell from './components/InputEditableCell';
import AntdTable from '@/components/AntdTable';
import ExtendedField from './components/ExtendedField';
import useSyncState from '@/hooks/useSyncState';
import { DataCollectionElementType } from '@/constants/aiDataCollection';

// inherits all attributes of antd table
interface IProps {
  className?: string;
  columns: any[];
  dataSource: any[];
  dataCollectionElementType?: DataCollectionElementType;
  onChange: (dataSource: any[]) => void;
}

export default memo<IProps>((props) => {
  const { className, columns: defaultColumns, onChange, ...restProps } = props;
  const { styles, cx } = useStyles();
  // The currently edited line
  const [curRecord, setCurRecord, getCurRecord] = useSyncState<any>(null);

  const onRow = (record: any) => {
    return {
      onClick: () => {
        setCurRecord(record);
      },
    };
  };

  const rowClassName = (record: any) => {
    return record?.custom_data_id === curRecord?.custom_data_id ? 'selected-row' : '';
  };

  const components = {
    body: {
      cell: EditableCell,
    },
  };

  const save = (values: { [key: string]: any }) => {
    onChange({ ...getCurRecord(), ...values });
  };

  const columns = useMemo(() => {
    return defaultColumns.map((col) => {
      if (!col.editable) {
        return col;
      }
      return {
        ...col,
        onCell: (record) => ({
          record,
          editable: col.editable,
          dataIndex: col.dataIndex,
          title: col.title,
          value: record[col.dataIndex],
          save,
        }),
      };
    });
  }, [defaultColumns]);

  return (
    <div className={cx(styles.columnInfo, className)}>
      <AntdTable
        sticky
        bordered
        className={cx(styles.table, styles.columnInfoTable)}
        components={components}
        rowClassName={rowClassName}
        pagination={false}
        columns={columns}
        subHeight={40}
        onRow={onRow}
        {...restProps}
      />
      {curRecord && restProps.dataCollectionElementType === DataCollectionElementType.TABLE && (
        <div className={styles.otherInfo}>
          <ExtendedField record={curRecord} save={save} />
        </div>
      )}
    </div>
  );
});
