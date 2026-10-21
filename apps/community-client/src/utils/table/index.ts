// Measure the rendered text and clamp its width between 60 and 200 pixels.
export function getTextWidth(text, moreSpace) {
  if (!text) return 60;
  // Create an off-screen div to measure text width
  const div = document.createElement('div');
  div.style.position = 'absolute';
  div.style.whiteSpace = 'nowrap';
  div.style.visibility = 'hidden';
  div.style.font = '14px';
  div.textContent = text;
  document.body.appendChild(div);
  const width = div.offsetWidth + moreSpace;
  document.body.removeChild(div);

  // Make sure the width is between 60 and 200
  if (width > 200) return 200;
  if (width < 60) return 60;
  return width;
}

const getCellDisplayValue = (cell) => {
  if (cell && typeof cell === 'object' && 'value' in cell) {
    return cell.value;
  }
  return cell;
};

// Determine column widths from header names and result data.
export function getTableWidth(queryResultData) {
  const tableWidth: any = [];
  queryResultData?.headerList?.map((item) => {
    tableWidth.push(getTextWidth(item.name, 44));
  });

  // Intercept the first 10 data of queryResultData.dataList
  const dataList = queryResultData?.dataList?.slice(0, 10);
  dataList?.map((item) => {
    item.map((value, index) => {
      const width = getTextWidth(getCellDisplayValue(value), 0);
      if (tableWidth[index] < width) {
        tableWidth[index] = width;
      }
    });
  });
  return tableWidth;
}
