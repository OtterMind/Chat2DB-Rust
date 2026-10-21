import React, { memo, useState } from 'react';
import { useStyles } from './style';
import { Dropdown, Button, Input, Space } from 'antd';
import { PlusOutlined, DeleteOutlined, CheckOutlined } from '@ant-design/icons';

interface EnumItem {
  key: string;
  value: string;
}

interface IProps {
  className?: string;
  children: React.ReactNode;
  value?: EnumItem[];
  onChange?: (value: EnumItem[]) => void;
}

export default memo<IProps>((props) => {
  const { className, children, value = [], onChange } = props;
  const { styles, cx } = useStyles();
  const [inputValue, setInputValue] = useState('');
  const [isAdding, setIsAdding] = useState(false);
  const [enumList] = useState<EnumItem[]>([]);

  const handleAdd = () => {
    setIsAdding(true);
    setInputValue('');
  };

  const handleConfirmAdd = () => {
    if (inputValue.trim()) {
      const newValue = [...value, { key: inputValue, value: inputValue }];
      onChange?.(newValue);
      setIsAdding(false);
    }
  };

  const handleDelete = (key: string) => {
    const newValue = value.filter((item) => item.key !== key);
    onChange?.(newValue);
  };

  const items = [
    {
      key: 'add',
      label: isAdding ? (
        <Space.Compact style={{ width: '100%' }}>
          <Input
            placeholder="请输入值"
            value={inputValue}
            onChange={(e) => setInputValue(e.target.value)}
            onPressEnter={handleConfirmAdd}
          />
          <Button type="primary" icon={<CheckOutlined />} onClick={handleConfirmAdd} />
        </Space.Compact>
      ) : (
        <Button type="text" icon={<PlusOutlined />} onClick={handleAdd} style={{ width: '100%' }}>
          添加枚举
        </Button>
      ),
    },
    ...value.map((item) => ({
      key: item.key,
      label: (
        <Space style={{ width: '100%', justifyContent: 'space-between' }}>
          <span>{item.value}</span>
          <Button type="text" danger icon={<DeleteOutlined />} onClick={() => handleDelete(item.key)} />
        </Space>
      ),
    })),
  ];

  const renderDropdown = () => {
    return (
      <div className={styles.dropdownContent}>
        {enumList.map((item) => (
          <div key={item.key}>
            {/* <Input value={item.key} onChange={(e) => handleChange(item.key, e.target.value)} />
            <Input value={item.value} onChange={(e) => handleChange(item.key, e.target.value)} /> */}
          </div>
        ))}
      </div>
    );
  };

  return (
    <div className={cx(styles.container, className)}>
      <Dropdown menu={{ items }} trigger={['click']} dropdownRender={renderDropdown}>
        {children}
      </Dropdown>
    </div>
  );
});
