import React, { memo, useEffect, useState, useRef, useMemo } from 'react';
import { useStyles } from './style';
import { Settings } from 'lucide-react';
import { Icon, IconButton, IconfontSvg } from '@chat2db/ui';
import ChartCard, { ChartCardRef } from '@/blocks/BI/ChartCard';
import { updateChart } from '@/service/dashboard';
import EditorChartModal, { EditChartModalRef } from '@/blocks/BI/ChartCardBox/EditorChartModal';
import i18n from '@/i18n';
import DingChartModal, { DingChartModalRef } from './DingChartModal';
import { IChartItem } from '@/typings/dashboard';
import { onExportToImage } from '@/utils/file';
import { AnswerParts } from '@/typings/chat';
import { chartDetailNormalization } from '../utils/dataTreating';
import { useFetchData, RefreshRule } from '../hooks/useFetchData';

interface IProps extends React.HTMLAttributes<HTMLDivElement> {
  className?: string;
  showDing?: boolean;
  // drop-down menu props
  dropdownProps?: any;
  // Choose one of chartId and chartDetail
  // Chart id
  chartId?: number;
  // Chart
  chartDetail?: IChartItem;
  // have editing permissions?
  isEditPermission?: boolean;
  // callback when the pop-up window is confirmed
  submitEditorChartCallback?: (data: AnswerParts) => void;
  refreshRule?: RefreshRule;
}

export default memo<IProps>((props) => {
  const {
    className,
    chartId,
    chartDetail: extraneousChartDetail,
    dropdownProps: extraneousDropdownProps,
    showDing,
    isEditPermission = true,
    submitEditorChartCallback: extraneousSubmitEditorChartCallback,
    refreshRule,
    ...rest
  } = props;

  const { styles, cx, theme } = useStyles();
  const EditorChartModalRef = useRef<EditChartModalRef>(null);
  // Chart
  const [chartDetail, setChartDetail] = useState<IChartItem>();
  const dingChartModalRef = useRef<DingChartModalRef>(null);
  const chartCardRef = useRef<ChartCardRef>(null);
  const { chartDetail: fetchChartDetail } = useFetchData({
    chartId,
    refreshRule,
  });

  useEffect(() => {
    if (fetchChartDetail) {
      setChartDetail(fetchChartDetail);
    }
  }, [fetchChartDetail]);

  useEffect(() => {
    if (extraneousChartDetail) {
      setChartDetail(extraneousChartDetail);
    }
  }, [extraneousChartDetail]);

  const submitEditorChartCallback = (_chartDetail) => {
    extraneousSubmitEditorChartCallback?.(_chartDetail);
    setChartDetail(_chartDetail);
    if (chartId) {
      void updateChart(chartDetailNormalization(_chartDetail));
    }
  };

  const dropdownProps = useMemo(() => {
    const settings = {
      key: 'settings',
      label: i18n('dashboard.chart.setting'),
      icon: <Icon size="sm" icon={Settings} />,
      onClick: () => {
        EditorChartModalRef.current?.controlEditChartModal('editChart', chartDetail);
      },
    };
    const exportToImage = {
      key: 'exportToImage',
      label: i18n('dashboard.export2image'),
      icon: <IconfontSvg code="icon-export-to-image" size="sm" />,
      onClick: () => {
        setTimeout(() => {
          onExportToImage(chartCardRef.current?.getChartBodyRef(), chartDetail?.chartSchema?.title || 'Chart', {
            backgroundColor: theme.colorBgBase,
          });
        }, 0);
      },
    };
    const defaultDropdownProps = {
      menu: {
        items: [settings, exportToImage, ...(extraneousDropdownProps?.menu?.items || [])],
      },
    };
    return defaultDropdownProps;
    // }, [chartDetail, chartId, EditorChartModalRef, extraneousDropdownProps, editableChart]);
  }, [chartDetail, chartId, EditorChartModalRef, extraneousDropdownProps]);

  const dingChart = () => {
    if (!chartDetail) return;
    dingChartModalRef.current?.openModal(chartDetail);
  };

  const extendAction = useMemo(() => {
    if (showDing) {
      return <IconButton code="icon-ding" size="sm" onClick={dingChart} />;
    }
    return false;
  }, [chartDetail]);

  const handleEditTextOnBlur = (text) => {
    const data = { ...chartDetail, chartSchema: { ...chartDetail?.chartSchema, title: text } };
    submitEditorChartCallback(data);
    setChartDetail(data);
  };

  return (
    <>
      <ChartCard
        className={cx(styles.chartCard, className)}
        chartDetail={chartDetail}
        isEditPermission={isEditPermission}
        dropdownProps={dropdownProps}
        extendAction={extendAction}
        editTextOnBlur={handleEditTextOnBlur}
        // errorComment={errorComment}
        ref={chartCardRef}
        {...rest}
      />

      <EditorChartModal
        // editableChart={editableChart}
        submitEditorChartCallback={submitEditorChartCallback}
        ref={EditorChartModalRef}
      />
      <DingChartModal ref={dingChartModalRef} />
    </>
  );
});
