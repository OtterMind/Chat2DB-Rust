import { memo } from 'react';
import { useStyles } from './style';
import { Dropdown, Checkbox } from 'antd';
import { i18n } from '@/i18n';
import { DataCollectionElementType } from '@/constants/aiDataCollection';

interface IProps {
  className?: string;
  dataCollectionElementType?: DataCollectionElementType;
  selectedTable: string[];
  setSelectedTable: (selectedTable: string[]) => void;
}

export default memo<IProps>((props) => {
  const { className, dataCollectionElementType, selectedTable, setSelectedTable } = props;
  const { styles, cx } = useStyles();

  const selectedTableLength = selectedTable.length || 0;

  const handleChange = (value) => {
    setSelectedTable(selectedTable.filter((item) => item !== value));
  };

  const dropdownRender = () => {
    return (
      <div className={styles.dropdownRender}>
        <div className={styles.dropdownRenderTitle}>
          {i18n(
            'workspace.tips.selectedTable',
            selectedTableLength,
            dataCollectionElementType === DataCollectionElementType.VIEW
              ? i18n('common.text.views')
              : i18n('common.text.table'),
          )}
        </div>
        <div className={styles.dropdownRenderBody}>
          {selectedTable.map((item, index) => {
            return (
              <Checkbox
                key={index}
                checked={true}
                onChange={() => {
                  handleChange(item);
                }}
              >
                {item}
              </Checkbox>
            );
          })}
        </div>
      </div>
    );
  };

  return (
    <div className={cx(className, styles.selectiveDisplay)}>
      <div>
        {i18n(
          'workspace.tips.selectedTable',
          selectedTableLength,
          dataCollectionElementType === DataCollectionElementType.VIEW
            ? i18n('common.text.views')
            : i18n('common.text.table'),
        )}
      </div>
      <Dropdown destroyPopupOnHide dropdownRender={dropdownRender} placement="topLeft">
        <div className={styles.showAll}>{i18n('workspace.tips.showAll', selectedTableLength)}</div>
      </Dropdown>
    </div>
  );
});
