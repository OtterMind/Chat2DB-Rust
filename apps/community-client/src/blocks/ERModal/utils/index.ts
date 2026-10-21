import ELK from 'elkjs/lib/elk.bundled.js';
import { ConnectionLineType, type Edge } from '@xyflow/react';
import { IERTableDetail } from '@/typings/er';

export interface IStoredLayout {
  nodeList?: {
    id: string;
    position: {
      x: number;
      y: number;
    };
  }[];
  virtualEdges?: {
    id: string;
    source: string;
    target: string;
    sourceHandle?: string;
    targetHandle?: string;
  }[];
}

// Create ELK instance
const elk = new ELK();

// Add function to calculate actual node size
const calculateNodeSize = (tableData: any) => {
  const ROW_HEIGHT = 40; // Height of each row
  const HEADER_HEIGHT = 50; // Meter head height
  const MIN_WIDTH = 300; // minimum width

  // Calculate the number of table rows (number of columns)
  const rowCount = tableData.columnList.length;

  // Calculate actual height
  const height = HEADER_HEIGHT + rowCount * ROW_HEIGHT;

  // Calculate width - allows for more precise calculations based on content length
  const width = MIN_WIDTH;

  return { width, height };
};

// Modify createElkNodes function
const createElkNodes = (nodes: IERTableDetail[]) => {
  return nodes.map((item) => {
    const { width, height } = calculateNodeSize(item);
    return {
      id: item.name,
      width,
      height,
    };
  });
};

const createEdgeId = (foreignKey: any) => 
  `${foreignKey.pkTableName}_${foreignKey.pkColumnName}-${foreignKey.fkTableName}_${foreignKey.fkColumnName}`;

const getCommonEdgeProps = (foreignKey: any) => ({
  id: createEdgeId(foreignKey),
  sourceHandle: `${foreignKey.pkTableName}_${foreignKey.pkColumnName}`,
  targetHandle: `${foreignKey.fkTableName}_${foreignKey.fkColumnName}`,
});

const createElkEdges = (data: IERTableDetail[]) => {
  const edges: any[] = [];
  data.forEach((table) => {
    table?.foreignKeyList?.forEach((foreignKey) => {
      edges.push({
        ...getCommonEdgeProps(foreignKey),
        sources: [foreignKey.pkTableName],
        targets: [foreignKey.fkTableName],
      });
    });
  });
  return edges;
};

// Calculate the layout with ELK.
export const getLaidOutElements = async (nodes: any[], edges: Edge[]) => {
  const elkNodes = createElkNodes(nodes);
  const elkEdges = createElkEdges(nodes);

  const elkGraph = {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.spacing.nodeNode': '100',
      'elk.layered.spacing.nodeNodeBetweenLayers': '200',
      'elk.direction': 'RIGHT',
      'elk.padding': '[50,50,50,50]',
      'elk.layered.crossingMinimization.strategy': 'LAYER_SWEEP',
      'elk.layered.nodePlacement.strategy': 'NETWORK_SIMPLEX',
      'elk.layered.spacing.edgeEdgeBetweenLayers': '50',
      'elk.layered.spacing.edgeNodeBetweenLayers': '50',
      'elk.layered.mergeEdges': 'true',
      'elk.layered.spacing': '100',
      'elk.spacing.componentComponent': '100',
    },
    children: elkNodes,
    edges: elkEdges,
  };

  const layoutedGraph = await elk.layout(elkGraph);

  // Apply the positions calculated by ELK to the nodes.
  const laidOutNodes = layoutedGraph.children!.map((node, index) => ({
    id: node.id,
    type: 'erModalTable',
    data: { tableDetail: nodes[index] },
    position: { x: node.x || 0, y: node.y || 0 },
  }));

  return { nodes: laidOutNodes, edges };
};

export const getEdgesFromData = (data: any[], storedLayout: IStoredLayout | null) => {
  const edges: Edge[] = [];

  storedLayout?.virtualEdges?.forEach((edge) => {
    edges.push({
      ...(edge || {}),
      style: {
        strokeWidth: 2,
        strokeDasharray: '5 5',
      },
    });
  });

  data.forEach((table) => {
    table?.foreignKeyList?.forEach((foreignKey) => {
      edges.push({
        ...getCommonEdgeProps(foreignKey),
        source: foreignKey.pkTableName,
        target: foreignKey.fkTableName,
        type: ConnectionLineType.Bezier,
      });
    });
  });

  return edges;
};

export const trimLayoutPositions = (nodes: any[]) => {
  const nodeList = nodes.map((node) => ({
    id: node.id,
    position: node.position,
  }));

  return nodeList;
};

export const parseStoredLayout = (nodes?: string | null): IStoredLayout | null => {
  let _nodes = null;
  if (!nodes) {
    return _nodes;
  }
  try {
    _nodes = JSON.parse(nodes);
  } catch (error) {
    console.error('getStoredLayout error', error);
  }
  return _nodes;
};
