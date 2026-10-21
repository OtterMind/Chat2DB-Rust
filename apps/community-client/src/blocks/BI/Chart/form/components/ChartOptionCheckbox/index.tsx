/*
 * ThemeColorSelect
 * report theme color selector
 */
import { memo } from 'react';
// import { useStyles } from './style';
import { Checkbox, Col, Row } from 'antd';
import i18n from '@/i18n';
import { ChartType } from '../../../constants';

interface IProps {
  value: any;
  onChange: any;
  chartType: ChartType;
}

const ChartOptionCheckbox = (props: IProps) => {
  const { value, onChange, chartType } = props;
  return (
    <Checkbox.Group value={value} onChange={onChange} style={{ width: '100%' }}>
      <Row>
        <Col span={12}>
          <Checkbox value="showLegend" style={{ lineHeight: '32px' }}>
            {i18n('dashboard.chart.chartOption.showLegend')}
          </Checkbox>
        </Col>
        <Col span={12}>
          <Checkbox value="showLabel" style={{ lineHeight: '32px' }}>
            {i18n('dashboard.chart.chartOption.showLabel')}
          </Checkbox>
        </Col>
        {![ChartType.Pie, ChartType.Funnel].includes(chartType) && (
          <>
            <Col span={12}>
              <Checkbox value="showAxisLine" style={{ lineHeight: '32px' }}>
                {i18n('dashboard.chart.chartOption.showAxisLine')}
              </Checkbox>
            </Col>
            <Col span={12}>
              <Checkbox value="showSplitLine" style={{ lineHeight: '32px' }}>
                {i18n('dashboard.chart.chartOption.showSplitLine')}
              </Checkbox>
            </Col>
          </>
        )}
        {[ChartType.Line, ChartType.AreaLine, ChartType.SmoothLine, ChartType.StepLine].includes(chartType) && (
          <>
            <Col span={12}>
              <Checkbox value="showSymbol" style={{ lineHeight: '32px' }}>
                {i18n('dashboard.chart.chartOption.symbol')}
              </Checkbox>
            </Col>
          </>
        )}
      </Row>
    </Checkbox.Group>
  );
};

export default memo(ChartOptionCheckbox);
