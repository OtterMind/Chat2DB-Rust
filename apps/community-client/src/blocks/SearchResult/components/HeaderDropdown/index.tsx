import React, { useState, memo, useImperativeHandle, forwardRef, useMemo, useEffect, useRef } from 'react';
import { Dropdown } from 'antd';
import { i18n } from '@/i18n';
import { copyToClipboard } from '@/utils';

interface IProps {
  className?: string;
}

type HeaderDropdownInfo = {
  event: React.MouseEvent;
  headerData: any;
  columnData: string[];
} | null;

export interface HeaderDropdownRef {
  setHeaderInfo: (info: HeaderDropdownInfo) => void;
}

const HeaderDropdown = (_props: IProps, ref) => {
  const [headerInfo, setHeaderInfo] = useState<HeaderDropdownInfo>(null);
  const headerInfoRef = useRef<HeaderDropdownInfo>(null);

  useEffect(() => {
    headerInfoRef.current = headerInfo;
  }, [headerInfo]);

  const rightConfig = useMemo(() => {
    return [
      {
        key: 'copyName',
        labelProps: {
          label: i18n('workspace.menu.copyColumnName'),
        },
        onClick: () => {
          copyToClipboard(headerInfoRef.current?.headerData?.name || '');
        },
      },
      {
        key: 'copyData',
        labelProps: {
          label: i18n('workspace.menu.copyColumnData'),
        },
        onClick: () => {
          copyToClipboard(headerInfoRef.current?.columnData || [], 'vertical');
        },
      },
      {
        key: 'copyNameAndData',
        labelProps: {
          label: i18n('workspace.menu.copyColumnNameAndData'),
        },
        onClick: () => {
          copyToClipboard(
            [headerInfoRef.current?.headerData?.name, ...(headerInfoRef.current?.columnData || [])],
            'vertical',
          );
        },
      },
    ];
  }, []);

  const menu = useMemo(() => {
    if (!headerInfo) {
      return {
        items: [],
        style: { display: 'none' },
      };
    }
    const dropdownsItems = rightConfig.map((item) => {
      return {
        key: item.key,
        onClick: () => {
          item.onClick?.();
        },
        // icon: item.labelProps.icon && <IconfontSvg code={item.labelProps.icon} size="lg" />,
        label: item.labelProps.label,
        // children: item.children?.map((t) => {
        //   return {
        //     key: t.key,
        //     onClick: () => {
        //       t.onClick?.();
        //     },
        //     icon: t.labelProps.icon && <IconfontSvg code={t.labelProps.icon} size="lg" />,
        //     label: t.labelProps.label,
        //   };
        // }),
      };
    });
    return {
      items: dropdownsItems,
      style: dropdownsItems?.length ? {} : { display: 'none' }, // is only displayed if there are menu items
    };
  }, [headerInfo]);

  useImperativeHandle(ref, () => ({
    setHeaderInfo,
  }));

  return (
    <Dropdown
      menu={menu}
      trigger={['click']}
      open={!!headerInfo}
      destroyPopupOnHide={true}
      onOpenChange={(next) => {
        if (!next) {
          setHeaderInfo(null);
        }
      }}
    >
      <div
        style={{
          position: 'fixed',
          left: headerInfo?.event?.clientX,
          top: headerInfo?.event?.clientY,
          height: 1,
          pointerEvents: 'none',
        }}
      />
    </Dropdown>
  );
};

export default memo(forwardRef<HeaderDropdownRef, IProps>(HeaderDropdown));
