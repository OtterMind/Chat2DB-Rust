import classNames from 'classnames';
import style from './index.less';

interface IProps {
  className: string;
}

function LayoutBasic({ className }: IProps) {
  return (
    <div className={classNames(style.layoutBasic, className)}>
      <div />
    </div>
  );
}

export default LayoutBasic;
