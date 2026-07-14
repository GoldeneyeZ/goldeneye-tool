// @vitest-environment node
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ProjectCard } from "../components/ProjectCard";

describe("untrusted graph metadata escaping", () => {
  it("lets React escape project names and filesystem paths", () => {
    const html = renderToStaticMarkup(
      <ProjectCard
        project={{
          name: '<img src=x onerror="alert(1)">',
          root_path: "</p><script>alert(2)</script>",
          indexed_at: "",
        }}
        schema={null}
        onSelect={() => undefined}
      />,
    );
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("<img src=x");
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("&lt;img src=x");
  });

  it("contains no raw HTML injection sink in production source", () => {
    const source = fileURLToPath(new URL("../", import.meta.url));
    const pending = [source];
    const violations: string[] = [];
    while (pending.length) {
      const directory = pending.pop();
      if (!directory) break;
      for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const file = path.join(directory, entry.name);
        if (entry.isDirectory()) pending.push(file);
        else if (/\.(ts|tsx)$/.test(entry.name) && !entry.name.includes(".test.")) {
          const text = readFileSync(file, "utf8");
          if (/dangerouslySetInnerHTML|\.innerHTML\s*=|\beval\s*\(/.test(text)) violations.push(file);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});
