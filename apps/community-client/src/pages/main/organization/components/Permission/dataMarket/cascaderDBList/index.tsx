import { FC, useState } from 'react';
import { useStyles } from './style';
import CascaderDB, { DBField } from '../cascaderDB';
import { Button } from 'antd';
import { IConnectionListItem } from '@/typings/connection';
import { DeleteOutlined } from '@ant-design/icons';

interface CascaderDBListProps {
  isPreview?: boolean;
  defaultValue?: DBField[];
  onChange?: (value: DBField[]) => void;
  dataSourceList?: IConnectionListItem[] | null;
}

const CascaderDBList: FC<CascaderDBListProps> = ({ dataSourceList, onChange, defaultValue }) => {
  const { styles } = useStyles();
  const [value, setValue] = useState<DBField[]>(defaultValue || [{}]);

  const handleDelete = (index: number) => {
    value.splice(index, 1);
    setValue([...value]);
    onChange && onChange([...value]);
  };

  return (
    <div className={styles.container}>
      {(value || []).map((item, index) => {
        const { dataSourceId, databaseName, tableName } = item;
        const cascaderValue = [dataSourceId, databaseName, tableName];
        return (
          <div className={styles.cascaderDiv} key={`${dataSourceId}-${databaseName}-${tableName}-${index}`}>
            <CascaderDB
              value={cascaderValue}
              dataSourceList={dataSourceList}
              onChange={(_, selectedOptions) => {
                const {
                  dataSourceId: selectedDataSourceId,
                  databaseName: selectedDatabaseName,
                  schemaName: selectedSchemaName,
                  tableName: selectedTableName,
                } = selectedOptions[selectedOptions.length - 1];
                value[index] = {
                  dataSourceId: selectedDataSourceId,
                  databaseName: selectedDatabaseName,
                  schemaName: selectedSchemaName,
                  tableName: selectedTableName,
                };
                setValue([...value]);
                onChange && onChange([...value]);
              }}
            />
            <Button
              className={styles.cascaderDelete}
              danger
              type="primary"
              icon={<DeleteOutlined />}
              onClick={() => handleDelete(index)}
            />
          </div>
        );
      })}
      <Button
        type="primary"
        onClick={() => {
          setValue([...value, {}]);
          onChange && onChange([...value, {}]);
        }}
      >
        添加数据源
      </Button>
    </div>
  );
};

export default CascaderDBList;
