import { useMemo } from 'react';
import { useStyles } from './style';
import { Menu, MenuProps } from 'antd';
import i18n from '@/i18n';
import { KnowledgeManagementPromptType } from '@/constants/knowledgeManagement';

interface IProps {
  menuKey: NavType;
  onClickMenu: (key: NavType) => void;
}

type MenuItem = Required<MenuProps>['items'][number];

export enum NavType {
  // Terminology.
  KNOWLEDGE_TERM = KnowledgeManagementPromptType.KNOWLEDGE_TERM,
  // Business logic explanations.
  BUSINESS_LOGIC = KnowledgeManagementPromptType.BUSINESS_LOGIC,
  // Case optimization.
  SQL_TEMPLATE = KnowledgeManagementPromptType.SQL_TEMPLATE,
  // Table annotations.
  ANNOTATION_TABLE = 'ANNOTATION_TABLE',
}

const OrgNavTypeList = ({ menuKey, onClickMenu }: IProps) => {
  const { styles } = useStyles();

  const items: MenuItem[] = useMemo(
    () => [
      {
        key: NavType.KNOWLEDGE_TERM,
        label: i18n('knowledgeManagement.nav.terminology'),
      },
      {
        key: NavType.BUSINESS_LOGIC,
        label: i18n('knowledgeManagement.nav.businessLogic'),
      },
      {
        key: NavType.SQL_TEMPLATE,
        label: i18n('knowledgeManagement.nav.caseOptimization'),
      },
      // {
      //   key: NavType.ANNOTATION_TABLE,
      //   label: i18n('knowledgeManagement.nav.annotationTable'),
      // },
    ],
    [],
  );

  return (
    <div className={styles.wrapper}>
      <div className={styles.title}>{i18n('knowledgeManagement.title')}</div>
      <div className={styles.menuBox}>
        <Menu
          defaultOpenKeys={[NavType.KNOWLEDGE_TERM]}
          selectedKeys={[menuKey]}
          mode="inline"
          items={items}
          className={styles.menuWrapper}
          onClick={(e) => {
            onClickMenu(e.key as unknown as NavType);
          }}
        />
      </div>
    </div>
  );
};

export default OrgNavTypeList;
