import SplitPane from 'react-split-pane';
import OrgNavList, { OrgNavType } from './components/OrgNavList';
import OrgContainer from './components/OrgContainer';
import { useOrgStore } from '@/store/organization';

const Organization = () => {
  const { orgNav, setOrgNav } = useOrgStore((s) => ({
    orgNav: s.orgNav,
    setOrgNav: s.setOrgNav,
  }));

  const handleMenuChange = (key: OrgNavType) => {
    setOrgNav(key);
  };

  return (
    <SplitPane size={220} pane2Style={{ width: '0px' }} minSize={220} maxSize={300} split="vertical" primary="first">
      <OrgNavList onClickMenu={handleMenuChange} menuKey={orgNav} />
      <OrgContainer menuKey={orgNav} />
    </SplitPane>
  );
};

export default Organization;
