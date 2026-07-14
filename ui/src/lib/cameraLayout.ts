import type { GraphNode } from "./types";

export interface CameraFrame {
  position: [number, number, number];
  lookAt: [number, number, number];
}

export function computeCameraFrame(
  nodes: ReadonlyArray<GraphNode>,
  ids: ReadonlySet<number>,
): CameraFrame | null {
  if (ids.size === 0) return null;

  let cx = 0;
  let cy = 0;
  let cz = 0;
  let count = 0;
  for (const node of nodes) {
    if (!ids.has(node.id)) continue;
    cx += node.x;
    cy += node.y;
    cz += node.z;
    count++;
  }
  if (count === 0) return null;

  cx /= count;
  cy /= count;
  cz /= count;

  let maxDistance = 0;
  for (const node of nodes) {
    if (!ids.has(node.id)) continue;
    const distance = Math.sqrt(
      (node.x - cx) ** 2 + (node.y - cy) ** 2 + (node.z - cz) ** 2,
    );
    maxDistance = Math.max(maxDistance, distance);
  }

  const distance = Math.max(count <= 5 ? 300 : 200, maxDistance * 3);
  return {
    lookAt: [cx, cy, cz],
    position: [cx + distance * 0.2, cy + distance * 0.15, cz + distance],
  };
}
