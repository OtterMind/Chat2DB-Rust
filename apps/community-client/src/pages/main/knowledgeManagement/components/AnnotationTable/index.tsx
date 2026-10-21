import { memo } from 'react';
import { useStyles } from './style';
import PageTitle from '@/components/PageTitle';
import i18n from '@/i18n';
import Description from '@/components/Description';
// import NewTree from '@/blocks/NewTree';

interface IProps {
  className?: string;
}

export default memo<IProps>((props) => {
  const { className } = props;
  const { styles, cx } = useStyles();
  return (
    <div className={cx(styles.container, className)}>
      <div className={styles.header}>
        <PageTitle title={i18n('knowledgeManagement.nav.annotationTable')} />
        <Description className={styles.description}>
          {i18n('knowledgeManagement.annotationTable.description')}
        </Description>
      </div>
      <div className={styles.treeContainer}>
        {/* <NewTree className={styles.tree} />
        <div className={styles.treeContent}>1</div> */}
      </div>
    </div>
  );
});
