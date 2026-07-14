// @vitest-environment node
import { describe, expect, it } from "vitest";
import { CAMERA_LAYOUT_FIXTURE } from "../fixtures/cameraLayout";
import { computeCameraFrame } from "../lib/cameraLayout";

describe("deterministic camera layout fixtures", () => {
  it("frames stable server coordinates exactly", () => {
    const ids = new Set<number>(CAMERA_LAYOUT_FIXTURE.selected);
    expect(computeCameraFrame(CAMERA_LAYOUT_FIXTURE.nodes, ids)).toEqual(
      CAMERA_LAYOUT_FIXTURE.expected,
    );
  });

  it("is independent of response order and ignores unknown IDs", () => {
    const reordered = [...CAMERA_LAYOUT_FIXTURE.nodes].reverse();
    const ids = new Set<number>([999, ...CAMERA_LAYOUT_FIXTURE.selected]);
    expect(computeCameraFrame(reordered, ids)).toEqual(CAMERA_LAYOUT_FIXTURE.expected);
    expect(computeCameraFrame(reordered, new Set([999]))).toBeNull();
  });
});
