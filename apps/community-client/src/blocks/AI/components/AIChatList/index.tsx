import { useStyles } from './style';

export interface IProps {
  // currently streaming output?
  isStreaming?: boolean;
}

const AIChatList = (_props: IProps) => {
  const { styles } = useStyles();

  return <div className={styles.container}>AIChatList</div>;
};

export default AIChatList;
