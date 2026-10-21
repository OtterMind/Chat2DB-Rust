import { MonacoEditor, MonacoEditorRef } from '@/components/SQLEditor';
import { Button, Flex } from 'antd';
import { useRef } from 'react';

const EditorTest = () => {
  const editorRef = useRef<MonacoEditorRef>(null);

  return (
    <Flex vertical gap={5} style={{ height: '100vh' }}>
      <Flex gap={5}>
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
            editorRef.current?.setValue('   SELECT * FROM cursor;', 'cursor');
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
      </Flex>

      <div style={{ flex: 1 }}>
        <MonacoEditor ref={editorRef} id={'1'} />
      </div>
    </Flex>
  );
};

export default EditorTest;
