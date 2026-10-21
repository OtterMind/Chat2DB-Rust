import { memo } from 'react';
import { useStyles } from './style';
import { i18n } from '@/i18n';
import { useTreeStore } from '@/store/tree';
import { DivProps } from '@/typings/common';
import { Empty, EmptyImage } from '@chat2db/ui';

interface IProps extends DivProps {}

export default memo<IProps>(({ className }) => {
  const { styles, cx } = useStyles();

  const { searchResult } = useTreeStore((state) => ({
    searchResult: state.searchResult,
  }));

  return (
    <div className={cx(styles.noConnectionBox, className)}>
      <div className={cx(styles.noConnectionContent)}>
        <Empty
          title={
            searchResult?.length === 0 ? i18n('workspace.tips.noSearchResult') : i18n('workspace.tips.noConnection')
          }
          image={EmptyImage.DBList}
        />
      </div>
    </div>
  );
});
