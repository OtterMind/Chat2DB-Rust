import React, { memo, useRef, forwardRef, ForwardedRef, useImperativeHandle } from 'react';
import { MoreVertical, ShieldX } from 'lucide-react';
import { EditText, Icon, IconButton, Loading } from '@chat2db/ui';
import { DropDownProps as AntdDropDownProps, Dropdown } from 'antd';
import Chart from '@/blocks/BI/Chart';
import { DivProps } from '@/typings/common';
import { useStyles } from './style';
import { IChartItem } from '@/typings';

export interface ChartCardProps extends DivProps {
  /**
   * @description Custom style
   * @default false
   */
  className?: string;
  /**
   * @description selected state
   * @default false
   */
  active?: boolean;
  /**
   * @description dropdown attribute,
   */
  dropdownProps?: AntdDropDownProps;
  /**
   * @description whether it is selected,
   */
  /**
   * @description Empty status prompt
   */
  emptyComment?: React.ReactNode;
  /**
   * @description Edit name out of focus
   */
  editTextOnBlur?: (text: string) => void;
  /**
   * @description extended operation
   */
  extendAction?: React.ReactNode;
  /**
   * @description error status prompt
   */
  errorComment?: React.ReactNode;
  // have editing permissions?
  isEditPermission?: boolean;

  /**
   * @description ChartDetails
   */
  chartDetail?: IChartItem;
}

export interface ChartCardRef {
  getChartBodyRef: () => any;
}

const ChartCard = forwardRef((props: ChartCardProps, ref: ForwardedRef<ChartCardRef>) => {
  const {
    className,
    dropdownProps,
    emptyComment,
    editTextOnBlur,
    extendAction,
    errorComment,
    isEditPermission = true,
    chartDetail,
    ...rest
  } = props;
  const { styles, cx } = useStyles();
  const { chartSchema } = chartDetail || {};
  const chartBodyRef = useRef<any>(null);

  useImperativeHandle(ref, () => ({
    getChartBodyRef: () => {
      return chartBodyRef;
    },
  }));

  return (
    <div className={cx(styles.chatCard, className)} {...rest}>
      <div className={cx(styles.header)}>
        <div className={styles.title}>
          {editTextOnBlur ? (
            <EditText hoverShowBorder onBlur={editTextOnBlur} disabledEdit={!isEditPermission}>
              {chartSchema?.title || chartSchema?.summary || ''}
            </EditText>
          ) : (
            chartSchema?.title || ''
          )}
        </div>
        {isEditPermission && (
          <div className={styles.action}>
            {extendAction}
            {!!dropdownProps?.menu?.items?.length && (
              <Dropdown {...dropdownProps} trigger={['click']}>
                <IconButton size="sm" icon={MoreVertical} />
              </Dropdown>
            )}
          </div>
        )}
      </div>
      <div className={styles.body} ref={chartBodyRef}>
        {errorComment ? (
          <div className={styles.errorComment}>
            <Icon size={50} icon={ShieldX} />
            <div>{errorComment}</div>
          </div>
        ) : (
          <>{chartDetail ? <Chart chartDetail={chartDetail} emptyComment={emptyComment} /> : <Loading />}</>
        )}
      </div>
    </div>
  );
});

export default memo(ChartCard);
