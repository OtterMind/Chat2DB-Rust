import { useStyles } from './style';
import AIHeader from '../AIChatHeader';
import AIChatList from '../AIChatList';
const AIContainer = () => {
  const { styles } = useStyles();

  return (
    <div className={styles.container}>
      <AIHeader />
      <AIChatList />
    </div>
  );
};

export default AIContainer;
