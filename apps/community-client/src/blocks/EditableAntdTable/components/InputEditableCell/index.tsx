import { InputRef, Input, Checkbox } from 'antd';
import React, { useEffect, useRef, useState } from 'react';
import { useStyles } from './style';
import { useUpdateEffect } from 'ahooks';

export const enum EditableCellType {
  INPUT = 'input',
  SELECT = 'select',
  CHECKBOX = 'checkbox',
}

interface Item {
  key: string;
  name: string;
  age: string;
  address: string;
}

interface EditableCellProps extends React.PropsWithChildren<any> {
  editable: EditableCellType;
  dataIndex: keyof Item;
  save: (params: { [key: string]: any }) => void;
}

const InputEditableCell: React.FC<React.PropsWithChildren<EditableCellProps>> = ({
  editable,
  children,
  dataIndex,
  save,
  ...restProps
}) => {
  const [editing, setEditing] = useState(false);
  const inputRef = useRef<InputRef>(null);
  const { styles, cx } = useStyles();
  const [value, setValue] = useState<any>(restProps.value);

  useEffect(() => {
    if (editing) {
      inputRef.current?.focus();
    }
  }, [editing]);

  useUpdateEffect(() => {
    save({ [dataIndex]: value });
  }, [value]);

  const renderEditableCell = () => {
    if (editable === EditableCellType.INPUT) {
      if (!editing) {
        return (
          <div
            className={(cx('editable-cell-value-wrap'), styles.tableCell)}
            onClick={() => {
              setEditing(true);
            }}
          >
            {children}
          </div>
        );
      }
      return (
        <Input
          className={styles.input}
          ref={inputRef}
          value={value}
          onChange={(e) => {
            setValue(e.target.value);
          }}
          onPressEnter={() => {
            setEditing(false);
          }}
          onBlur={() => {
            setEditing(false);
          }}
        />
      );
    }

    if (editable === EditableCellType.CHECKBOX) {
      return (
        <Checkbox
          className={styles.checkbox}
          checked={value}
          onChange={(e) => {
            setValue(e.target.checked);
          }}
        />
      );
    }

    return <div className={styles.tableCell}>{children}</div>;
  };

  return <td {...restProps}>{renderEditableCell()}</td>;
};

export default InputEditableCell;
