// Held-out test for the `terax-feature-removal` task (removal of the editor's
// Vim mode). The grader copies this file to
// `src/modules/settings/agent-bench-feature-removal.test.ts` in the worktree,
// runs it with the repository's Vitest and deletes it again. It is not named
// `*.test.ts` here so that `node --test` in tools/agent-bench does not pick it up.
//
// It checks behaviour, not text: the preference is no longer part of the
// defaults, is not loaded from a settings file that still contains it, is not
// forwarded to subscribers and has no setter; the neighbouring editor
// preferences and the editor's shared extensions still work.
import { Compartment, EditorState } from "@codemirror/state";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({
  entries: [] as [string, unknown][],
  sets: [] as [string, unknown][],
  emitted: [] as { event: string; payload: unknown }[],
  onChange: null as ((key: string, value: unknown) => void) | null,
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock("@tauri-apps/plugin-store", () => ({
  LazyStore: class {
    async entries() {
      return tauri.entries;
    }
    async get(key: string) {
      return tauri.entries.find(([k]) => k === key)?.[1];
    }
    async has(key: string) {
      return tauri.entries.some(([k]) => k === key);
    }
    async set(key: string, value: unknown) {
      tauri.sets.push([key, value]);
    }
    async delete() {
      return true;
    }
    async save() {}
    async onChange(cb: (key: string, value: unknown) => void) {
      tauri.onChange = cb;
      return () => {};
    }
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: async (event: string, payload: unknown) => {
    tauri.emitted.push({ event, payload });
  },
  listen: async (
    event: string,
    cb: (event: { payload: unknown }) => void,
  ) => {
    tauri.listeners.set(event, cb);
    return () => {};
  },
}));

import * as extensions from "../editor/lib/extensions";
import * as store from "./store";

const REMOVED_KEY = "vimMode";

beforeEach(() => {
  tauri.entries = [];
  tauri.sets = [];
  tauri.emitted = [];
  tauri.onChange = null;
  tauri.listeners.clear();
});

describe("removed preference", () => {
  it("is not part of the defaults and has no setter", () => {
    expect(Object.keys(store.DEFAULT_PREFERENCES)).not.toContain(REMOVED_KEY);
    expect(Object.keys(store)).not.toContain("setVimMode");
  });

  it("is not loaded from a settings file that still has it", async () => {
    tauri.entries = [
      [REMOVED_KEY, true],
      ["editorWordWrap", true],
      ["editorFontSize", 15],
      ["editorAutoSave", true],
    ];
    const prefs = await store.loadPreferences();
    expect(Object.keys(prefs)).not.toContain(REMOVED_KEY);
    expect(Object.keys(prefs).sort()).toEqual(
      Object.keys(store.DEFAULT_PREFERENCES).sort(),
    );
    expect(prefs.editorWordWrap).toBe(true);
    expect(prefs.editorFontSize).toBe(15);
    expect(prefs.editorAutoSave).toBe(true);
    expect(tauri.sets.map(([key]) => key)).not.toContain(REMOVED_KEY);
  });

  it("is not forwarded to preference subscribers", async () => {
    const seen: [string, unknown][] = [];
    await store.onPreferencesChange((key, value) => seen.push([key, value]));
    const fromEvent = [...tauri.listeners.values()];
    expect(tauri.onChange).not.toBeNull();
    expect(fromEvent.length).toBeGreaterThan(0);

    tauri.onChange?.(REMOVED_KEY, true);
    for (const cb of fromEvent) {
      cb({ payload: { key: REMOVED_KEY, value: true } });
    }
    expect(seen).toEqual([]);

    tauri.onChange?.("editorWordWrap", true);
    for (const cb of fromEvent) {
      cb({ payload: { key: "editorWordWrap", value: false } });
    }
    expect(seen).toEqual([
      ["editorWordWrap", true],
      ["editorWordWrap", false],
    ]);
  });
});

describe("neighbouring editor preferences", () => {
  it("keep their defaults", () => {
    expect(store.DEFAULT_PREFERENCES).toMatchObject({
      editorFontSize: 13,
      editorWordWrap: false,
      editorAutoSave: false,
      editorAutoSaveDelay: 1000,
      editorFormatOnSave: false,
      editorFormatter: "lsp",
      autocompleteEnabled: false,
      showHidden: false,
      terminalCursorBlink: false,
    });
  });

  it("fall back to the defaults on an empty settings file", async () => {
    const prefs = await store.loadPreferences();
    expect(prefs.editorWordWrap).toBe(false);
    expect(prefs.editorAutoSaveDelay).toBe(1000);
    expect(prefs.editorFormatter).toBe("lsp");
  });

  it("are still written and broadcast by their setters", async () => {
    await store.setEditorWordWrap(true);
    await store.setEditorAutoSave(true);
    expect(tauri.sets).toEqual([
      ["editorWordWrap", true],
      ["editorAutoSave", true],
    ]);
    expect(tauri.emitted.map((e) => e.payload)).toEqual([
      { key: "editorWordWrap", value: true },
      { key: "editorAutoSave", value: true },
    ]);
  });
});

describe("editor extensions shared by every editor pane", () => {
  const compartments = [
    "languageCompartment",
    "readOnlyCompartment",
    "wrapCompartment",
    "lspCompartment",
    "indentCompartment",
  ] as const;

  it("still export the compartments the other features reconfigure", () => {
    for (const name of compartments) {
      expect(extensions[name], name).toBeInstanceOf(Compartment);
    }
    expect(Object.keys(extensions)).not.toContain("vimCompartment");
  });

  it("still build an editor state that can be reconfigured", () => {
    const state = EditorState.create({
      doc: "a\n\tb",
      extensions: [
        extensions.wrapCompartment.of([]),
        ...extensions.buildSharedExtensions(),
        extensions.indentCompartment.of(extensions.DEFAULT_INDENT),
        extensions.languageCompartment.of([]),
        extensions.lspCompartment.of([]),
      ],
    });
    expect(state.doc.lines).toBe(2);
    expect(state.tabSize).toBe(2);
    const next = state.update({
      effects: extensions.indentCompartment.reconfigure(
        extensions.indentExtension("\t"),
      ),
    }).state;
    expect(next.tabSize).toBe(4);
  });
});
