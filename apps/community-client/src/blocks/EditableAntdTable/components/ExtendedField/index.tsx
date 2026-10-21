import { memo, useEffect } from 'react';
import { useStyles } from './style';
import { Form, Input } from 'antd';
// import EditEnum from '@/components/EditEnum';
// import { IconButton } from '@chat2db/ui';
import i18n from '@/i18n';

interface IProps {
  className?: string;
  save: (data: any) => void;
  record: any;
}

export default memo<IProps>((props) => {
  const { className, save, record } = props;
  const { styles, cx } = useStyles();
  const [form] = Form.useForm();

  useEffect(() => {
    if (record) {
      form.setFieldsValue({
        foreignColumnName: record.foreignColumnName,
        foreignTableName: record.foreignTableName,
        // columnEnumMap: record.columnEnumMap && JSON.stringify(record.columnEnumMap),
      });
    }
  }, [record]);

  const handleSave = () => {
    const values = form.getFieldsValue();
    // let columnEnumMap = undefined;
    // try {
    //   columnEnumMap = JSON.parse(values.columnEnumMap);
    // } catch (error) {
    //   console.error('columnEnumMap parsing failed', error);
    // }
    save({
      ...values,
      // columnEnumMap,
    });
  };

  return (
    <div className={cx(styles.container, className)}>
      <div className={styles.title}>{i18n('workspace.menu.extraAttribute')}</div>
      <Form form={form}>
        <Form.Item name="foreignTableName" label={i18n('workspace.menu.foreignKeyTable')}>
          <Input onBlur={handleSave} onPressEnter={handleSave} />
        </Form.Item>
        <Form.Item name="foreignColumnName" label={i18n('workspace.menu.foreignKeyColumn')}>
          <Input onBlur={handleSave} onPressEnter={handleSave} />
        </Form.Item>
        {/* <Form.Item className={styles.enumItem} name="columnEnumMap" label={i18n('workspace.menu.enum')}>
          <div className={styles.enumContainer}>
            <Input
              placeholder='[{"1":"succeed"},{"2":"fail"}]'
              addonBefore={
                <EditEnum>
                  <IconButton size={{ boxSize: 32, iconSize: 24 }} code="icon-add" />
                </EditEnum>
              }
              onBlur={handleSave}
              onPressEnter={handleSave}
            />
          </div>
        </Form.Item> */}
      </Form>
    </div>
  );
});
