import { z } from "zod";

const rawNumberSchema = z.preprocess((value) => {
  if (typeof value === "string" && value.trim() !== "") {
    return Number(value);
  }
  return value;
}, z.number().finite());

export const rawSearchItemSchema = z
  .object({
    qualified_name: z.string().optional(),
    qn: z.string().optional(),
    name: z.string().optional(),
    label: z.string().optional(),
    type: z.string().optional(),
    labels: z.array(z.string()).optional(),
    file_path: z.string().nullish(),
    file: z.string().nullish(),
    path: z.string().nullish(),
    start_line: rawNumberSchema.nullish(),
    line: rawNumberSchema.nullish(),
    signature: z.string().optional(),
  })
  .passthrough();

export const rawSearchObjectResponseSchema = z
  .object({
    results: z.array(rawSearchItemSchema).optional(),
    semantic_results: z.array(rawSearchItemSchema).optional(),
    matches: z.array(rawSearchItemSchema).optional(),
  })
  .passthrough();

export const rawSearchResponseSchema = z.union([
  z.array(rawSearchItemSchema),
  rawSearchObjectResponseSchema,
]);

export const rawArchitectureResponseSchema = z.record(z.unknown());

export const rawProjectSchema = z
  .object({
    name: z.string().optional(),
    project: z.string().optional(),
    project_id: z.string().optional(),
    id: z.string().optional(),
    root_path: z.string().optional(),
    rootPath: z.string().optional(),
    path: z.string().optional(),
  })
  .passthrough();

export const rawProjectsResponseSchema = z.union([
  z.array(rawProjectSchema),
  z.object({ projects: z.array(rawProjectSchema) }).passthrough(),
]);

export const rawSnippetSchema = z
  .object({
    qualified_name: z.string().optional(),
    qn: z.string().optional(),
    name: z.string().optional(),
    label: z.string().optional(),
    type: z.string().optional(),
    labels: z.array(z.string()).optional(),
    file_path: z.string().optional(),
    file: z.string().optional(),
    path: z.string().optional(),
    start_line: rawNumberSchema.optional(),
    line: rawNumberSchema.optional(),
    end_line: rawNumberSchema.optional(),
    lines: rawNumberSchema.optional(),
    complexity: rawNumberSchema.optional(),
    cognitive: rawNumberSchema.optional(),
    visibility: z.string().optional(),
    signature: z.string().optional(),
    return_type: z.string().optional(),
    decorators: z.string().optional(),
    callers: rawNumberSchema.optional(),
    callees: rawNumberSchema.optional(),
    code: z.string().optional(),
    source: z.string().optional(),
    snippet: z.string().optional(),
    content: z.string().optional(),
    text: z.string().optional(),
  })
  .passthrough();

export const rawSnippetManifestSchema = rawSnippetSchema.extend({
  source_bytes: rawNumberSchema,
  source_lines: rawNumberSchema,
  source_sha256: z.string(),
  indexed_file_hash: z.string(),
  chunk_bytes: rawNumberSchema,
  chunk_count: rawNumberSchema,
});

export const rawSnippetChunkSchema = rawSnippetManifestSchema.extend({
  source: z.string(),
  chunk: rawNumberSchema,
  chunk_start_byte: rawNumberSchema,
  chunk_end_byte: rawNumberSchema,
  eof: z.boolean(),
  truncated: z.boolean(),
});
