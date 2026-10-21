import { useReactFlow, getNodesBounds, getViewportForBounds } from '@xyflow/react';
import { toPng } from 'html-to-image';
import { IconfontSvg } from '@chat2db/ui';

function downloadImage(dataUrl) {
  const a = document.createElement('a');

  a.setAttribute('download', 'reactflow.png');
  a.setAttribute('href', dataUrl);
  a.click();
}

// minimum size limit
const MIN_WIDTH = 1024;
const MIN_HEIGHT = 768;
// margin
const PADDING = 100;

function DownloadButton({ reactFlowKey }: { reactFlowKey: string }) {
  const { getNodes } = useReactFlow();
  const onClick = () => {
    const nodes = getNodes();
    const nodesBounds = getNodesBounds(nodes);

    // Calculate the appropriate image size, ensuring all nodes are included and margins added
    const width = Math.max(MIN_WIDTH, nodesBounds.width + PADDING * 2);
    const height = Math.max(MIN_HEIGHT, nodesBounds.height + PADDING * 2);

    // calculates appropriate scaling and position
    const viewport = getViewportForBounds(nodesBounds, width, height, 0.5, 2, PADDING);

    const flowContainer = document.querySelector(`#${reactFlowKey} .react-flow__viewport`);
    if (!flowContainer) {
      console.error('not found flow container');
      return;
    }

    toPng(flowContainer as HTMLElement, {
      width,
      height,
      style: {
        width: `${width}px`,
        height: `${height}px`,
        transform: `translate(${viewport.x}px, ${viewport.y}px) scale(${viewport.zoom})`,
      },
    }).then(downloadImage);
  };

  return (
    <div
      onClick={onClick}
      style={{ width: 18, height: 18, display: 'flex', alignItems: 'center', justifyContent: 'center' }}
    >
      <IconfontSvg style={{ width: 18, height: 18, maxWidth: 18, maxHeight: 18 }} size={16} code="icon-download" />
    </div>
  );
}

export default DownloadButton;
