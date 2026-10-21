import { memo } from 'react';
// import { IconButton } from '@chat2db/ui';
import { useStyles } from './style';
import { Dropdown, type MenuProps } from 'antd';
import { refreshPage } from '@/utils';
import { history } from 'umi';
import { Platform } from '@/constants/os';
import jcefApi from '@/jcef';
// import { JcefEventBus, JavaPushActionType } from '@/jcef/eventBus';

interface AppBarProps {
  className?: string;
}

const AppBar = memo<AppBarProps>(({ className }) => {
  const { styles, cx } = useStyles();
  // const [isMaximized, setIsMaximized] = useState(false);

  // useLayoutEffect(() => {
  //   // Monitor window status changes
  //   const handleWindowStateChange = (maximized: boolean) => {
  //     setIsMaximized(maximized);
  //   };

  //   JcefEventBus.on(JavaPushActionType.IS_WINDOW_MAXIMIZED, handleWindowStateChange);

  //   // Get the initial window state
  //   jcefApi?.isWindowMaximized().then((maximized: boolean) => {
  //     setIsMaximized(maximized);
  //   });

  //   return () => {
  //     JcefEventBus.off(JavaPushActionType.IS_WINDOW_MAXIMIZED);
  //   };
  // }, []);

  const items: MenuProps['items'] = [
    {
      key: '1',
      label: 'Open log',
      onClick: () => {
        jcefApi?.openLog();
      },
    },
    {
      key: '2',
      label: 'Open the console',
      onClick: () => {
        jcefApi?.openDevTools();
      },
    },
    {
      key: '3',
      label: 'Refresh app',
      onClick: refreshPage,
    },
    {
      key: '4',
      label: 'test-jcef',
      onClick: () => {
        history.push('/test-jcef');
      },
    },
    {
      key: '5',
      label: 'go-back-home',
      onClick: () => {
        history.push('/');
      },
    },
  ];

  const handleDoubleClick = async () => {
    jcefApi?.handleDoubleClickAppBar();
  };

  // const handelMinimizeWindow = (e) => {
  //   e.stopPropagation();
  //   jcefApi?.minimizeWindow();
  // };

  // const handelMaximizeWindow = (e) => {
  //   e.stopPropagation();
  //   const handleApi = isMaximized ? jcefApi?.minimizeWindow : jcefApi?.maximizeWindow;
  //   handleApi?.();
  // };

  // const handelCloseWindow = (e) => {
  //   e.stopPropagation();
  //   jcefApi?.closeWindow();
  // };

  if (!window.navigator.os_type || window.navigator.os_type !== Platform.Mac) {
    // const showLeftContainer = checkIsSharePage();
    // if (__WEBAPP__ && !isEmbedIframe && !showLeftContainer) {
    //   window._appTitleBarHeight = 36;
    //   return (
    //     <div style={{ height: 36, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
    //       {i18n('common.text.pleaseDownloadClient')}
    //       <Button type="link" href={appUrlConfig.DOWNLOAD_URL} target="_blank">
    //         {i18n('common.button.download')}
    //       </Button>
    //     </div>
    //   );
    // }
    window._appTitleBarHeight = 0;
    return <></>;
  } else {
    // TODO: jcef
    // window._appTitleBarHeight = jcefApi.getPlatform() === Platform.Mac ? 30 : 36;
    window._appTitleBarHeight = 30;
  }

  // When testing appBar on the web side, comment out the if else code above and open the comment code below.
  // window._appTitleBarHeight = 36;

  return (
    <div
      className={cx(styles.appBar, { [styles.windowsAppBar]: window.navigator.os_type !== Platform.Mac }, className)}
      onDoubleClick={handleDoubleClick}
    >
      <div className={styles.logoContainer}>
        {window.navigator.os_type !== Platform.Mac ? (
          <Dropdown destroyPopupOnHide menu={{ items }} trigger={['click']} className={styles.dropdown}>
            <div className={styles.appName}>Chat2DB</div>
          </Dropdown>
        ) : (
          <div className={styles.appName}>Chat2DB</div>
        )}
      </div>
      {/* {window.navigator.os_type !== Platform.Mac && (
        <div className={styles.windowsActionBar}>
          <IconButton className={styles.windowsAction} code="icon-minus" onClick={handelMinimizeWindow} />
          <IconButton
            className={styles.windowsAction}
            code={isMaximized ? 'icon-unmaximize' : 'icon-maximize'}
            onClick={handelMaximizeWindow}
          />
          <IconButton
            className={cx(styles.windowsAction, styles.closeAction)}
            code="icon-close"
            onClick={handelCloseWindow}
          />
        </div>
      )} */}
    </div>
  );
});

export default AppBar;
