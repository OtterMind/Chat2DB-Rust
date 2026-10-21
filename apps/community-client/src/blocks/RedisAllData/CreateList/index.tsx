import {
  memo,
  useMemo,
  useState,
  forwardRef,
  ForwardedRef,
  useImperativeHandle,
  useEffect,
  useRef,
} from 'react';
import feedback from '@/utils/feedback';
import { useStyles } from './style';
import BaseTable, { BaseTableRef } from '@/components/BaseTable';
import { ToolbarBtn } from '@chat2db/ui';
import { Flex } from 'antd';
import { RedisFieldType, ActionType } from '@/constants/redis';
// import { RedisDataItem } from '@/typings/redis';
import { v4 as uuid } from 'uuid';
import i18n from '@/i18n';

interface IProps {
  className?: string;
  type: RedisFieldType;
  originalValueList?: any[];
}

export interface CreateListRef {
  getValueList: () => any;
}

const CreateList = forwardRef((props: IProps, ref: ForwardedRef<CreateListRef>) => {
  const { className, type } = props;
  const { styles, cx } = useStyles();

  const [valueList, setValueList] = useState<any[]>([]);
  const [deleteValueList, setDeleteValueList] = useState<any[]>([]);
  const [selectedRows, setSelectedRows] = useState<number[]>([]);
  const baseTableRef = useRef<BaseTableRef>(null);

  useEffect(() => {
    setValueList([]);
  }, [type]);

  useEffect(() => {
    const _defaultValueList =
      props.originalValueList?.map((item) => {
        return {
          ...item,
          action: ActionType.ORIGINAL,
          id: uuid(),
        };
      }) || [];
    setValueList(_defaultValueList);
  }, [props.originalValueList]);

  // table column configuration
  const columns = useMemo(() => {
    switch (type) {
      case RedisFieldType.LIST:
      case RedisFieldType.SET:
        return [
          {
            title: 'Value',
            name: 'value',
          },
        ];
      case RedisFieldType.ZSET:
        return [
          {
            title: 'Value',
            name: 'value',
          },
          {
            title: 'Score',
            name: 'score',
            onlyNumber: true,
          },
        ];
      case RedisFieldType.HASH:
        return [
          {
            title: 'Field',
            name: 'field',
          },
          {
            title: 'Value',
            name: 'value',
          },
        ];
      default:
        return [
          {
            title: 'Value',
            name: 'value',
          },
        ];
    }
  }, [type]);

  // Get value list
  const getValueList = () => {
    if (valueList.length === 0 || valueList.every((item) => item.action === ActionType.DELETE)) {
      // interrupts code execution
      feedback.error(i18n('redis.message.valueListIsEmpty'));
      throw new Error('valueList is empty');
    }

    valueList.forEach((item, index) => {
      item.index = index;
      delete item.id;
    });
    switch (type) {
      case RedisFieldType.ZSET:
        return {
          zsValues: valueList,
        };
      case RedisFieldType.HASH:
        return {
          hashValues: valueList,
        };
      case RedisFieldType.SET:
        return {
          values: valueList,
        };
      default:
        return {
          listValues: [...valueList, ...deleteValueList],
        };
    }
  };

  // Methods exposed to parent components
  useImperativeHandle(ref, () => ({
    getValueList,
  }));

  // processing new
  const handleAdd = () => {
    switch (type) {
      case RedisFieldType.LIST:
      case RedisFieldType.SET:
        setValueList([
          ...valueList,
          {
            value: '',
            action: ActionType.ADD,
          },
        ]);
        break;
      case RedisFieldType.ZSET:
        setValueList([
          ...valueList,
          {
            value: '',
            score: '',
            action: ActionType.ADD,
          },
        ]);
        break;
      case RedisFieldType.HASH:
        setValueList([...valueList, { field: '', value: '', action: ActionType.ADD }]);
        break;
      default:
        setValueList([...valueList, { value: '', action: ActionType.ADD }]);
        break;
    }
    setTimeout(() => {
      baseTableRef.current?.scrollToBottom();
    }, 0);
  };

  // handle deletion
  const handleDelete = () => {
    if (selectedRows.length === 0) {
      return;
    }
    const deleteData = valueList[selectedRows[0]];

    if (deleteData.action === ActionType.ADD) {
      return;
    }

    setDeleteValueList([
      ...deleteValueList,
      {
        ...deleteData,
        action: ActionType.DELETE,
      },
    ]);

    const newValues = valueList.filter((item, index) => !selectedRows.includes(index));
    setSelectedRows([]);
    setValueList(newValues);
  };

  // cell modification
  const handleCellChange = (index, columnName, value) => {
    setValueList((prevTableData) => {
      const newData = [...prevTableData];
      // real subscript
      newData[index][columnName] = value;
      // If it is newly added data, the mark will remain unchanged after modification.
      if (newData[index].action === ActionType.ADD) {
        return newData;
      }
      newData[index].action = ActionType.UPDATE;
      return newData;
    });
  };

  // Move up
  const handleMoveUp = () => {
    if (selectedRows.length === 1) {
      const index = selectedRows[0];
      if (index === 0) {
        return;
      }
      const newValues = [...valueList];
      newValues[index].action = ActionType.UPDATE;
      [newValues[index - 1], newValues[index]] = [newValues[index], newValues[index - 1]];
      setValueList(newValues);
      setSelectedRows([index - 1]);
    }
  };

  // Move down
  const handleMoveDown = () => {
    if (selectedRows.length === 1) {
      const index = selectedRows[0];
      if (index === valueList.length - 1) {
        return;
      }
      const newValues = [...valueList];
      newValues[index].action = ActionType.UPDATE;
      [newValues[index + 1], newValues[index]] = [newValues[index], newValues[index + 1]];
      setValueList(newValues);
      setSelectedRows([index + 1]);
    }
  };

  return (
    <div className={cx(styles.createList, className)}>
      <BaseTable
        ref={baseTableRef}
        className={styles.baseTable}
        tableData={valueList}
        columns={columns}
        editable
        draggableColumn={false}
        showEmptyState={false}
        onChangeCell={handleCellChange}
        selectedRows={selectedRows}
        onSelectedRowsChange={setSelectedRows}
      />
      <Flex align="center" gap="4px" className={styles.operationLine}>
        <ToolbarBtn prefixIcon="icon-add" text={i18n('common.button.addNew')} size="sm" onClick={handleAdd} />
        <ToolbarBtn prefixIcon="icon-minus" text={i18n('common.button.delete')} size="sm" onClick={handleDelete} />
        {type === RedisFieldType.LIST && (
          <>
            <ToolbarBtn
              disabled={selectedRows.length !== 1 || valueList.length < 2 || selectedRows[0] === 0}
              prefixIcon="icon-move-up"
              text={i18n('common.button.moveUp')}
              size="sm"
              onClick={handleMoveUp}
            />
            <ToolbarBtn
              disabled={selectedRows.length !== 1 || valueList.length < 2 || selectedRows[0] === valueList.length - 1}
              prefixIcon="icon-move-down"
              text={i18n('common.button.moveDown')}
              size="sm"
              onClick={handleMoveDown}
            />
          </>
        )}
      </Flex>
    </div>
  );
});

export default memo(CreateList);
