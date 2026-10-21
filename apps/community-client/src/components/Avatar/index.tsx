import { useMemo } from 'react';
import i18n from '@/i18n';
import { useUserStore } from '@/store/user';
import Upload from '@/components/Upload';
import { Avatar, AvatarProps } from 'antd';
import { createStyles } from 'antd-style';
import { IconfontSvg } from '@chat2db/ui';
import { UploadTypeEnum } from '@/typings/upload';
import { useOrgStore } from '@/store/organization';
import { IOrganizationVO } from '@/typings/enterprise/organization';
import feedback from '@/utils/feedback';

export const useStyles = createStyles(({ css, token }, { size, avatar }: { size: number; avatar?: string }) => {
  const calcIconSize = () => {
    let fontSize = 12;
    if (size > 50) {
      fontSize = 20;
    } else if (size > 28) {
      fontSize = 14;
    } else {
      fontSize = 10;
    }
    return fontSize;
  };

  return {
    wrapper: css`
      position: relative;
      &:hover {
      }
    `,
    avatar: css`
      cursor: pointer;
      /* margin-bottom: 16px; */
      transition: scale 400ms ${token.motionEaseOut};
      background-color: ${avatar ? 'transparent' : token.colorPrimary};
      color: ${token.colorBgBase};
      font-size: ${calcIconSize()}px !important;
      &:hover {
        background-color: ${avatar ? 'transparent' : token.colorPrimaryHover};
      }
      &:active {
        scale: 0.9;
      }
    `,
    edit: css`
      position: absolute;
      padding: 2px;
      bottom: ${(1 / 23) * size - 96 / 23}px;
      right: ${(4 / 23) * size - 384 / 23}px;
      border-radius: 50%;
      opacity: 1;
      background-color: ${token.colorBgBase};
    `,
  };
});

interface IEditorAvatarProps extends AvatarProps {
  // className?: string;
  size?: number;
  /**
   * @description Whether editing is allowed.
   * @default false
   */
  canEditor?: boolean;
  /**
   * c
   */
  org?: IOrganizationVO;
}
const EditorAvatar = ({
  className,
  size = 50,
  canEditor = false,
  shape = 'circle',
  org,
  children,
}: IEditorAvatarProps) => {
  const { curUser, updateUser } = useUserStore((s) => ({
    curUser: s.curUser,
    updateUser: s.updateUser,
  }));
  const { curOrg, updateOrg } = useOrgStore((s) => ({
    curOrg: s.curOrg,
    updateOrg: s.updateOrg,
  }));

  const curAvatar = useMemo(
    () => ({
      avatar: org ? org?.organizationAvatar : curUser?.avatar,
      displayName: org ? org?.name : curUser?.displayName,
    }),
    [curUser, curOrg, org],
  );
  const { styles, cx } = useStyles({ size, avatar: curAvatar?.avatar });

  const beforeUpload = (file) => {
    const isLt1M = file.size / 1024 / 1024 < 1; // Under 1 MB.
    if (!isLt1M) {
      feedback.error(i18n('setting.update.tooLarge'));
    }
    return isLt1M;
  };

  const firstNameChar = useMemo(() => {
    if (curAvatar?.displayName) {
      const chars = [...curAvatar.displayName];
      return chars[0] || '';
    }
    return '';
  }, [curAvatar?.displayName]);

  const avatarDom = (
    <Avatar
      size={size}
      shape={shape}
      alt={i18n('setting.text.avatar')}
      src={curAvatar?.avatar}
      className={cx(styles.avatar, className)}
      style={{ verticalAlign: 'middle' }}
    >
      {!curAvatar?.avatar && firstNameChar}
    </Avatar>
  );

  if (!canEditor) {
    return avatarDom;
  }

  return (
    <Upload
      name="file"
      accept="image/*"
      showUploadList={false}
      beforeUpload={beforeUpload}
      uploadType={UploadTypeEnum.AVATOR}
      onChange={({ file }) => {
        if (file?.response?.res?.status === 200) {
          const { cdn, name } = file?.response || {};
          org ? updateOrg({ id: org.id, organizationAvatar: cdn + name }) : updateUser({ avatar: cdn + name });
        }
      }}
    >
      {children ? (
        children
      ) : (
        <div className={styles.wrapper}>
          {avatarDom}
          <IconfontSvg code="icon-edit" className={styles.edit} />
        </div>
      )}
    </Upload>
  );
};

export default EditorAvatar;
