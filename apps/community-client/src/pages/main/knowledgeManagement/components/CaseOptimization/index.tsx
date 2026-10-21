import { memo } from 'react';
import { useStyles } from './style';
import PageTitle from '@/components/PageTitle';
import i18n from '@/i18n';
import Description from '@/components/Description';

interface IProps {
  className?: string;
}

export default memo<IProps>((props) => {
  const { className } = props;
  const { styles, cx } = useStyles();
  return (
    <div className={cx(styles.container, className)}>
      <PageTitle title={i18n('knowledgeManagement.nav.caseOptimization')} />
      <Description>{i18n('knowledgeManagement.caseOptimization.description')}</Description>
    </div>
  );
});
