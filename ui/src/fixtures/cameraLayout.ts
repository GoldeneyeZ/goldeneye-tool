import type { GraphNode } from "../lib/types";

export const CAMERA_LAYOUT_FIXTURE = {
  nodes: [
    { id: 1, x: 0, y: 0, z: 0, label: "Function", name: "alpha", size: 1, color: "#fff" },
    { id: 2, x: 100, y: 0, z: 0, label: "Function", name: "beta", size: 1, color: "#fff" },
    { id: 3, x: 0, y: 100, z: 25, label: "File", name: "src", size: 1, color: "#fff" },
  ] satisfies GraphNode[],
  selected: [1, 2],
  expected: {
    lookAt: [50, 0, 0],
    position: [110, 45, 300],
  },
} as const;
