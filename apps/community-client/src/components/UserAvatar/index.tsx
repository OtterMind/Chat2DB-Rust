import { memo } from 'react';
import { Avatar } from 'antd';
import { useUserStore } from '@/store/user';

interface IProps {
  className?: string;
  size?: number;
}

export default memo<IProps>((props) => {
  const { className, size = 40 } = props;
  const { avatar, firstNameChar } = useUserStore((s) => {
    return {
      avatar: s.curUser?.avatar,
      firstNameChar: s.curUser?.displayName?.[0] || '',
    };
  });
  return (
    <Avatar className={className} size={size} src={avatar}>
      {!avatar && firstNameChar}
    </Avatar>
  );
});
