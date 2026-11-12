Promote the Tauri command bridge out of the AI module.

`src/modules/ai/lib/native.ts` wraps the backend commands (filesystem, git, shell, workspace) and exports them as an object called `native`, together with the result types. It is no longer AI-specific: source control, the explorer, the editor, git history and the app shell all reach into the AI module to import it, and `native` is too generic a name. Move it to the shared library folder and rename its main export.

Requirements:

1. Move the module to `src/lib/nativeBridge.ts`. Everything the old file exports (the bridge object and every exported type) is exported from the new file, with the same shape and behaviour.
2. Rename the exported object from `native` to `nativeBridge`. Its method names, parameters, return types and the backend commands they invoke do not change.
3. Update every usage in `src/`: value imports, type-only imports, every call made through the object, and the test mocks of the module. Every reference to the module must use the specifier `@/lib/nativeBridge`, including in files inside `src/modules/ai` that use a relative path today.
4. Leave nothing behind: delete `src/modules/ai/lib/native.ts`; no re-export shim at the old path or anywhere else; the new file must not also export the object as `native`; no import-side alias such as `import { nativeBridge as native }`.
5. This is a pure refactor. Do not change behaviour, do not modify Rust files, and do not touch unrelated code that merely uses the word "native" for something else.

The project must still type-check (`tsc --noEmit`) and the existing frontend tests must still pass.
