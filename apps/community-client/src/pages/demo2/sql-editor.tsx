import { useRef, useState } from 'react';
import { Button, Flex, Input, Select } from 'antd';
import SQLEditor, { SQLEditorRef } from '@/components/SQLEditor/editor/SQLEditor';
import { DatabaseTypeCode } from '@/constants';

const SQLEditorDemo = () => {
  const editorRef = useRef<SQLEditorRef>(null);
  const [isActive, setIsActive] = useState(false);
  const [dbInfo, setDbInfo] = useState({
    databaseType: DatabaseTypeCode.MYSQL,
    dataSourceId: 373,
    databaseName: 'enterprise_gateway_dev',
  });

  return (
    <Flex vertical gap={5} style={{ height: '100vh' }}>
      <Flex gap={5} align={'center'}>
        <Button
          onClick={() => {
            // setValue(value + '\nSELECT * FROM users;');
            editorRef.current?.setValue('\nSELECT * FROM users;');
          }}
        >
          末尾追加
        </Button>
        <Button
          onClick={() => {
            editorRef.current?.setValue('\nSELECT * FROM start;\n', 'start');
          }}
        >
          文首增加
        </Button>
        <Button
          onClick={() => {
            editorRef.current?.setValue('SELECT * FROM cursor;', 'cursor');
          }}
        >
          光标处增加
        </Button>
        <Button
          onClick={() => {
            editorRef.current?.setValue('select * from replace', 'replace');
          }}
        >
          替换
        </Button>
        <Button
          onClick={() => {
            editorRef.current?.setValue('', 'reset');
          }}
        >
          重置
        </Button>
        SQLEditorDemo
        <div>Editor is {isActive ? 'active' : 'inactive'}</div>
      </Flex>

      <Flex gap={5} align={'center'}>
        <Select
          value={dbInfo.databaseType}
          onChange={(value) => setDbInfo({ ...dbInfo, databaseType: value })}
          style={{ width: 120 }}
        >
          {Object.values(DatabaseTypeCode).map((type) => (
            <Select.Option key={type} value={type}>
              {type}
            </Select.Option>
          ))}
        </Select>
        <Input
          value={dbInfo.dataSourceId}
          onChange={(e) => setDbInfo({ ...dbInfo, dataSourceId: Number(e.target.value) })}
          style={{ width: 120 }}
          type="number"
        />
        <Input
          value={dbInfo.databaseName}
          onChange={(e) => setDbInfo({ ...dbInfo, databaseName: e.target.value })}
          style={{ width: 200 }}
        />
      </Flex>

      <div style={{ flex: 1 }}>
        <SQLEditor
          ref={editorRef}
          id={'sql-editor'}
          onMount={(editor) => {
            editor.onDidFocusEditorText(() => setIsActive(true));
            editor.onDidBlurEditorText(() => setIsActive(false));
          }}
          active={isActive}
          dbInfo={dbInfo}
        />
      </div>
    </Flex>
  );
};

export default SQLEditorDemo;
