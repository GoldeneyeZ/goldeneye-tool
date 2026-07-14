// @vitest-environment node
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const root = fileURLToPath(new URL("../../", import.meta.url));

describe("asset integrity and provenance", () => {
  it("matches every source checksum and manifest byte count", () => {
    const manifest = JSON.parse(
      readFileSync(path.join(root, "asset-manifest.json"), "utf8"),
    );
    for (const record of manifest.files) {
      const content = readFileSync(path.join(root, ...record.path.split("/")));
      expect(content.length, record.path).toBe(record.bytes);
      expect(createHash("sha256").update(content).digest("hex"), record.path).toBe(
        record.sha256,
      );
    }
    expect(manifest.upstream.commit).toBe(
      "2469ecc3a7a2f80debe296e1f17a1efcfdb9450c",
    );
  });

  it("keeps an exact checksum ledger for original upstream inputs", () => {
    const lines = readFileSync(path.join(root, "UPSTREAM_SOURCES.sha256"), "utf8")
      .trim()
      .split(/\r?\n/);
    expect(lines.length).toBeGreaterThan(40);
    for (const line of lines) {
      expect(line).toMatch(/^[0-9a-f]{64}  [^\\]+$/);
    }
  });
});
