import { createStyles } from 'antd-style';

export const useTableStyles = createStyles(({ css, token }) => {
  return {
    baseTable: css`
      height: 100%;
    `,
    supportBaseTableBox: css`
      height: 100%;
      overflow-y: auto;
      position: relative;
      ::-webkit-scrollbar {
        width: 8px;
      }
      .art-table {
        th {
          font-weight: 500;
          border-top: 0px !important;
        }
        th.first {
          border-left: 0;
        }
        th.last {
          border-right: 0px;
        }
        td.first {
          border-left: 0;
        }
        td.last {
          border-right: 0px;
        }
      }
      .art-table-cell {
        position: relative;
      }
      .art-table-row {
        height: 32px;
      }
      .art-table-header {
        background: transparent !important;
        overflow: hidden !important;
        top: 0px !important;
        .art-table-header-row {
          background-color: ${token.colorBgBase};
        }
      }
      .art-table-body {
        /* Track height 0, this scroll bar will conflict with the scroll bar at the bottom of the table */
        scrollbar-color: auto;
        ::-webkit-scrollbar {
          height: 0px !important;
        }
      }
      .art-sticky-scroll {
        /* The original style will have a margin-top: -9px; which is amazing, so I want to remake it to 0 here. */
        margin-top: 0px !important;
        scrollbar-color: auto;
        ::-webkit-scrollbar {
          height: 8px;
        }
        ::-webkit-scrollbar-thumb {
          background-color: ${token.colorFill};
        }
      }
      .art-table-body-scroll {
        padding-right: 6px;
      }
      .art-table-header-cell {
        padding: 0px 4px;
        > div {
          overflow: hidden;
        }
      }
      .sBxvO {
        width: 6px;
      }
      .art-table-wrapper {
        --font-size: 13px;
        --bgcolor: ${token.colorBgBase};
        --hover-bgcolor: ${token.colorFillTertiary};
        --header-bgcolor: ${token.colorFillQuaternary};
        --header-hover-bgcolor: ${token.colorFillSecondary};
        --highlight-bgcolor: ${token.colorFillTertiary};
        --header-highlight-bgcolor: ${token.colorFillSecondary};
        --color: ${token.colorText};
        --header-color: ${token.colorText};
        --lock-shadow: rgb(37 37 37 / 0.5) 0 0 6px 2px;
        --border-color: ${token.colorBorderSecondary};
        --cell-padding: 0px;
        --row-height: 32px;
        --lock-shadow: 0px 1px 2px 0px ${token.colorBorderSecondary};
      }

      /* The newly added empty state height is 100% */
      .empty {
        height: 100%;
        .art-loading-wrapper,
        .art-table-wrapper,
        .art-loading-content-wrapper,
        .art-table,
        table {
          height: 100%;
        }
        .art-table-body {
          height: calc(100% - 32px);
        }
        td.first {
          border-bottom: 0;
        }
      }
    `,
  };
});
