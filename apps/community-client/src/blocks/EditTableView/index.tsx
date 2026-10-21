import { memo, useMemo, useRef } from 'react';
import { useStyles } from './style';
import DynamicForm from '@/blocks/DynamicForm';
import { IDynamicFormItem } from '../DynamicForm/typings';
import { DynamicFormRef } from '../DynamicForm';
import { SQLEditor } from '@/components/SQLEditor';
import { v4 as uuidv4 } from 'uuid';

interface IProps {
  className?: string;
}

// using components
const config = [
  {
    labelName: '算法',
    name: 'algorithm',
    inputType: 'select',
    defaultValue: '3',
    required: false,
    multiple: false,
    display: null,
    selects: [
      {
        label: 'UNDEFINED',
        value: 0,
      },
      {
        label: 'MERGE',
        value: 1,
      },
      {
        label: 'TEMPTABLE',
        value: 2,
      },
      {
        label: null,
        value: 3,
      },
    ],
  },
  {
    labelName: '检查选项',
    name: 'checkOption',
    inputType: 'select',
    defaultValue: '2',
    required: false,
    multiple: false,
    display: null,
    selects: [
      {
        label: 'CASCADED',
        value: 0,
      },
      {
        label: 'LOCAL',
        value: 1,
      },
      {
        label: null,
        value: 2,
      },
    ],
  },
  {
    labelName: 'SQL 安全性',
    name: 'security',
    inputType: 'select',
    defaultValue: '2',
    required: false,
    multiple: false,
    display: null,
    selects: [
      {
        label: 'DEFINER',
        value: 0,
      },
      {
        label: 'INVOKER',
        value: 1,
      },
      {
        label: null,
        value: 2,
      },
    ],
  },
  {
    labelName: '视图名称',
    name: 'viewName',
    inputType: 'input',
    defaultValue: null,
    required: false,
    multiple: false,
    display: null,
    selects: null,
  },
  {
    labelName: '定义者',
    name: 'definer',
    inputType: 'input',
    defaultValue: null,
    required: false,
    multiple: false,
    display: null,
    selects: null,
  },
  {
    labelName: 'use or replace',
    name: 'useOrReplace',
    inputType: 'checkbox',
    defaultValue: 'false',
    required: false,
    multiple: false,
    display: null,
    selects: null,
  },
];

export default memo<IProps>((props) => {
  const { className } = props;
  const { styles, cx } = useStyles();
  const formRef = useRef<DynamicFormRef>(null);

  const sqlEditorId = useMemo(() => {
    return uuidv4();
  }, []);

  return (
    <div className={cx(styles.container, className)}>
      <DynamicForm ref={formRef} config={config as IDynamicFormItem[]} />
      <div className={styles.sqlEditor}>
        <SQLEditor
          id={sqlEditorId}
          dbInfo={{
            dataSourceId: 357,
            databaseName: 'er_modal',
            databaseType: 'MYSQL',
          }}
          action={() => {}}
        />
      </div>
      <button
        onClick={() => {
          console.log(formRef.current?.getFieldsValue());
        }}
      >
        获取表单值
      </button>
    </div>
  );
});
