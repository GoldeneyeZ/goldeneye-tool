#[cfg(any(feature = "compiled", test))]
pub use native::{
    NativePlan, WrapperSpec, compiler_include_path, derive_cobol_scanner_for_msvc,
    prepare_native_plan,
};

#[cfg(any(feature = "compiled", test))]
pub const STANDARD_SCANNER_SUFFIXES: [&str; 5] =
    ["create", "destroy", "scan", "serialize", "deserialize"];

#[cfg(any(feature = "compiled", test))]
#[must_use]
pub const fn missing_pack_remediation() -> &'static str {
    "GOLDENEYE_GRAMMAR_PACK_DIR is required for the compiled full grammar pack. \
Run: cargo xtask grammars sync --lock grammars/full-pack.toml \
--git-repo .upstream/codebase-memory-mcp \
--git-prefix internal/cbm/vendored/grammars \
--dest target/goldeneye-grammars"
}

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_COMPILED");
    println!("cargo:rerun-if-env-changed=GOLDENEYE_GRAMMAR_PACK_DIR");

    #[cfg(feature = "compiled")]
    if let Err(error) = native::compile_verified_pack() {
        eprintln!("goldeneye-full-grammars build failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(any(feature = "compiled", test))]
mod native {
    use std::fmt::Write as _;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[cfg(feature = "compiled")]
    use std::{env, ffi::OsString};

    use goldeneye_grammar_pack::{GrammarPackLock, verify_materialized_pack};
    use sha2::{Digest as _, Sha256};

    use super::STANDARD_SCANNER_SUFFIXES;

    #[cfg(feature = "compiled")]
    use super::missing_pack_remediation;

    const GENERATED_HEADER_PREFIX: &str = "// goldeneye-full-pack-lock-sha256: ";
    const EXPECTED_WRAPPER_COUNT: usize = 159;
    const EXPECTED_SCANNER_COUNT: usize = 102;
    const EXPECTED_ASSET_COUNT: usize = 914;
    const EXPECTED_NATIVE_SUPPORT_COUNT: usize = 1;
    const EXPECTED_DECLARED_LANGUAGE_COUNT: usize = 160;
    const EXPECTED_AVAILABLE_LANGUAGE_COUNT: usize = 159;
    const EXPECTED_RUNTIME_FACTORY_COUNT: usize = 157;
    const EXPECTED_ORPHAN_COUNT: usize = 2;
    const COBOL_SCANNER_SHA256: &str =
        "0e146beb0331e4f95e2fb815e263c649f2bc404b35dd1b19eb125cbd4ed95df8";
    const DERIVED_COBOL_SCANNER: &str = "goldeneye-derived-cobol-scanner.c";

    #[must_use]
    pub fn compiler_include_path(path: &Path) -> PathBuf {
        #[cfg(windows)]
        {
            let path = path.as_os_str().to_string_lossy();
            if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
                return PathBuf::from(format!(r"\\{unc}"));
            }
            if let Some(local) = path.strip_prefix(r"\\?\") {
                return PathBuf::from(local);
            }
        }
        path.to_owned()
    }

    /// Derive the pinned COBOL scanner variant required by MSVC's lack of VLAs.
    ///
    /// # Errors
    ///
    /// Returns an error unless the scanner hash and every compatibility
    /// precondition match exactly.
    pub fn derive_cobol_scanner_for_msvc(
        source_bytes: &[u8],
        expected_sha256: &str,
    ) -> Result<Vec<u8>, String> {
        let actual_sha256 = format!("{:x}", Sha256::digest(source_bytes));
        if actual_sha256 != expected_sha256 {
            return Err(format!(
                "COBOL scanner SHA-256 drift: expected {expected_sha256}, found {actual_sha256}"
            ));
        }
        let source = std::str::from_utf8(source_bytes)
            .map_err(|error| format!("verified COBOL scanner is not UTF-8: {error}"))?;
        let signature =
            "static bool start_with_word( TSLexer *lexer, char *words[], int number_of_words) {";
        let count_declaration = "const int number_of_comment_entry_keywords = 9;";
        let sole_call =
            "start_with_word(lexer, any_content_keyword, number_of_comment_entry_keywords)";
        let replacements = [
            (
                "char *keyword_pointer[number_of_words];",
                "char *keyword_pointer[9];",
            ),
            (
                "bool continue_check[number_of_words];",
                "bool continue_check[9];",
            ),
        ];

        require_occurrences(source, signature, 1, "start_with_word signature")?;
        require_occurrences(
            source,
            count_declaration,
            1,
            "nine-item COBOL keyword declaration",
        )?;
        require_occurrences(
            source,
            "start_with_word(",
            2,
            "definition plus exactly one call",
        )?;
        require_occurrences(
            source,
            sole_call,
            1,
            "exactly one call with the nine-item array",
        )?;
        require_occurrences(
            source,
            "number_of_comment_entry_keywords",
            2,
            "keyword count declaration plus sole call",
        )?;
        require_occurrences(source, "[number_of_words]", 2, "exactly two VLA bounds")?;

        let mut derived = source.to_owned();
        for (original, replacement) in replacements {
            require_occurrences(&derived, original, 1, "exact COBOL VLA declaration")?;
            derived = derived.replacen(original, replacement, 1);
        }
        if derived.contains("[number_of_words]") {
            return Err("COBOL scanner derivation left an unverified VLA bound".to_owned());
        }
        Ok(derived.into_bytes())
    }

    fn require_occurrences(
        source: &str,
        needle: &str,
        expected: usize,
        invariant: &str,
    ) -> Result<(), String> {
        let actual = source.matches(needle).count();
        if actual != expected {
            return Err(format!(
                "COBOL scanner precondition failed for {invariant}: expected {expected}, found {actual}"
            ));
        }
        Ok(())
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct WrapperSpec {
        ordinal: usize,
        grammar_name: String,
        exported_symbol: String,
        has_scanner: bool,
    }

    impl WrapperSpec {
        /// Build one deterministic wrapper specification from a locked record.
        ///
        /// # Errors
        ///
        /// Returns an error when the scanner language is not supported.
        pub fn new(
            ordinal: usize,
            grammar_name: &str,
            exported_symbol: &str,
            scanner_language: &str,
        ) -> Result<Self, String> {
            let has_scanner = match scanner_language {
                "none" => false,
                "c" => true,
                unsupported => {
                    return Err(format!(
                        "unsupported scanner language {unsupported:?} for grammar {grammar_name}"
                    ));
                }
            };
            Ok(Self {
                ordinal,
                grammar_name: grammar_name.to_owned(),
                exported_symbol: exported_symbol.to_owned(),
                has_scanner,
            })
        }

        #[must_use]
        pub fn grammar_name(&self) -> &str {
            &self.grammar_name
        }

        #[must_use]
        pub const fn has_scanner(&self) -> bool {
            self.has_scanner
        }

        #[must_use]
        pub fn wrapper_file_name(&self) -> String {
            format!("grammar_{:03}_{}.c", self.ordinal, self.grammar_name)
        }

        #[must_use]
        pub fn archive_name(&self) -> String {
            format!("goldeneye_full_grammar_{:03}", self.ordinal)
        }

        #[must_use]
        pub fn additional_include_paths(&self) -> Vec<PathBuf> {
            match self.grammar_name.as_str() {
                "cfml" => vec![PathBuf::from("cfml/tree_sitter")],
                "cobol" => vec![PathBuf::from("cobol")],
                _ => Vec::new(),
            }
        }

        #[must_use]
        pub fn render(&self) -> String {
            let mut source = String::new();
            source.push_str("/* Generated from the verified grammar lock; do not edit. */\n");
            source.push_str("#ifdef _MSC_VER\n#define restrict __restrict\n#endif\n");
            writeln!(
                source,
                "#define {} goldeneye_full_{}",
                self.exported_symbol, self.exported_symbol
            )
            .expect("writing to a String cannot fail");
            if self.has_scanner {
                for suffix in STANDARD_SCANNER_SUFFIXES {
                    let symbol = format!("{}_external_scanner_{suffix}", self.exported_symbol);
                    writeln!(source, "#define {symbol} goldeneye_full_{symbol}")
                        .expect("writing to a String cannot fail");
                }
            }
            writeln!(source, "#include \"{}/parser.c\"", self.grammar_name)
                .expect("writing to a String cannot fail");
            if self.has_scanner {
                if self.grammar_name == "cobol" {
                    writeln!(source, "#ifdef _MSC_VER").expect("writing to a String cannot fail");
                    writeln!(source, "#include \"{DERIVED_COBOL_SCANNER}\"")
                        .expect("writing to a String cannot fail");
                    writeln!(source, "#else").expect("writing to a String cannot fail");
                    writeln!(source, "#include \"cobol/scanner.c\"")
                        .expect("writing to a String cannot fail");
                    writeln!(source, "#endif").expect("writing to a String cannot fail");
                } else {
                    writeln!(source, "#include \"{}/scanner.c\"", self.grammar_name)
                        .expect("writing to a String cannot fail");
                }
            }
            source
        }
    }

    #[derive(Debug)]
    pub struct NativePlan {
        wrappers: Vec<WrapperSpec>,
        asset_paths: Vec<String>,
        lock_hash: String,
        declared_language_count: usize,
        available_language_count: usize,
        runtime_factory_count: usize,
        orphan_count: usize,
        native_support_count: usize,
    }

    impl NativePlan {
        #[must_use]
        pub fn wrappers(&self) -> &[WrapperSpec] {
            &self.wrappers
        }

        #[must_use]
        pub fn asset_paths(&self) -> &[String] {
            &self.asset_paths
        }

        fn require_production_inventory(&self) -> Result<(), String> {
            let scanner_count = self
                .wrappers
                .iter()
                .filter(|wrapper| wrapper.has_scanner())
                .count();
            let actual = (
                self.wrappers.len(),
                scanner_count,
                self.asset_paths.len(),
                self.declared_language_count,
                self.available_language_count,
                self.runtime_factory_count,
                self.orphan_count,
                self.native_support_count,
            );
            let expected = (
                EXPECTED_WRAPPER_COUNT,
                EXPECTED_SCANNER_COUNT,
                EXPECTED_ASSET_COUNT,
                EXPECTED_DECLARED_LANGUAGE_COUNT,
                EXPECTED_AVAILABLE_LANGUAGE_COUNT,
                EXPECTED_RUNTIME_FACTORY_COUNT,
                EXPECTED_ORPHAN_COUNT,
                EXPECTED_NATIVE_SUPPORT_COUNT,
            );
            if actual != expected {
                return Err(format!(
                    "full grammar inventory drift: expected {expected:?}, found {actual:?}"
                ));
            }
            Ok(())
        }
    }

    /// Verify the materialized pack and prepare its deterministic wrapper plan.
    ///
    /// # Errors
    ///
    /// Returns an error for lock, cache, generated-registry, or wrapper drift.
    pub fn prepare_native_plan(
        lock_path: &Path,
        generated_path: &Path,
        pack_root: &Path,
    ) -> Result<NativePlan, String> {
        let (lock, lock_hash) = GrammarPackLock::load_with_hash(lock_path)
            .map_err(|error| format!("failed to load grammar lock: {error}"))?;
        verify_materialized_pack(lock_path, &lock, pack_root)
            .map_err(|error| format!("failed to verify materialized grammar pack: {error}"))?;
        verify_generated_header(generated_path, &lock_hash)?;

        let mut grammar_records = lock.grammars.iter().collect::<Vec<_>>();
        grammar_records.sort_by(|left, right| left.name.cmp(&right.name));
        let wrappers = grammar_records
            .into_iter()
            .enumerate()
            .map(|(ordinal, grammar)| {
                WrapperSpec::new(
                    ordinal,
                    &grammar.name,
                    &grammar.exported_symbol,
                    &grammar.scanner_language,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut asset_paths = lock.locked_asset_paths().collect::<Vec<_>>();
        asset_paths.sort();

        Ok(NativePlan {
            wrappers,
            asset_paths,
            lock_hash,
            declared_language_count: lock.language_mappings.len(),
            available_language_count: lock.available_language_count(),
            runtime_factory_count: lock.unique_bound_grammar_count(),
            orphan_count: lock.orphan_grammar_names().len(),
            native_support_count: lock.native_support.len(),
        })
    }

    fn verify_generated_header(generated_path: &Path, lock_hash: &str) -> Result<(), String> {
        let generated = fs::read_to_string(generated_path).map_err(|error| {
            format!(
                "failed to read generated registry {}: {error}",
                generated_path.display()
            )
        })?;
        let first_line = generated.lines().next().unwrap_or_default();
        let expected = format!("{GENERATED_HEADER_PREFIX}{lock_hash}");
        if first_line != expected {
            return Err(format!(
                "generated registry lock hash does not match the active lock: expected {expected:?}, found {first_line:?}"
            ));
        }
        Ok(())
    }

    #[cfg(feature = "compiled")]
    pub fn compile_verified_pack() -> Result<(), String> {
        let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or_else(|| {
            "Cargo did not provide CARGO_MANIFEST_DIR to the build script".to_owned()
        })?);
        let workspace_root = manifest_dir
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .ok_or_else(|| {
                "full grammar crate is not nested under the workspace root".to_owned()
            })?;
        let pack_root = resolve_pack_root(workspace_root)?;
        let lock_path = workspace_root.join("grammars/full-pack.toml");
        let generated_path = manifest_dir.join("src/generated.rs");
        let plan = prepare_native_plan(&lock_path, &generated_path, &pack_root)?;
        plan.require_production_inventory()?;

        println!("cargo:rerun-if-changed={}", lock_path.display());
        println!("cargo:rerun-if-changed={}", generated_path.display());
        println!(
            "cargo:rerun-if-changed={}",
            pack_root
                .join(goldeneye_grammar_pack::PACK_STATE_FILE)
                .display()
        );
        for asset in plan.asset_paths() {
            println!("cargo:rerun-if-changed={}", pack_root.join(asset).display());
        }

        let out_dir = PathBuf::from(
            env::var_os("OUT_DIR")
                .ok_or_else(|| "Cargo did not provide OUT_DIR to the build script".to_owned())?,
        );
        let wrapper_root = out_dir.join(format!("goldeneye-full-wrappers-{}", plan.lock_hash));
        let target_is_msvc = env::var("CARGO_CFG_TARGET_ENV")
            .map_err(|error| format!("Cargo did not provide CARGO_CFG_TARGET_ENV: {error}"))?
            == "msvc";
        let derived_cobol_scanner = if target_is_msvc {
            let source_path = pack_root.join("cobol/scanner.c");
            let source = fs::read(&source_path).map_err(|error| {
                format!(
                    "failed to read verified COBOL scanner {}: {error}",
                    source_path.display()
                )
            })?;
            Some(derive_cobol_scanner_for_msvc(
                &source,
                COBOL_SCANNER_SHA256,
            )?)
        } else {
            None
        };
        fs::create_dir_all(&wrapper_root).map_err(|error| {
            format!(
                "failed to create wrapper directory {}: {error}",
                wrapper_root.display()
            )
        })?;
        if let Some(derived) = derived_cobol_scanner {
            write_if_changed(&wrapper_root.join(DERIVED_COBOL_SCANNER), &derived)?;
        }

        let compiler_options = supported_compiler_options()?;
        if compiler_options.is_msvc != target_is_msvc {
            return Err(format!(
                "compiler family disagrees with CARGO_CFG_TARGET_ENV: target msvc={target_is_msvc}, compiler msvc={}",
                compiler_options.is_msvc
            ));
        }
        for wrapper in plan.wrappers() {
            let wrapper_path = wrapper_root.join(wrapper.wrapper_file_name());
            let source = wrapper.render();
            write_if_changed(&wrapper_path, source.as_bytes())?;
            compile_wrapper(wrapper, &wrapper_path, &pack_root, &compiler_options);
        }
        Ok(())
    }

    #[cfg(feature = "compiled")]
    fn resolve_pack_root(workspace_root: &Path) -> Result<PathBuf, String> {
        let raw = env::var_os("GOLDENEYE_GRAMMAR_PACK_DIR")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| missing_pack_remediation().to_owned())?;
        let configured = PathBuf::from(raw);
        let candidate = if configured.is_absolute() {
            configured
        } else {
            workspace_root.join(configured)
        };
        candidate.canonicalize().map_err(|error| {
            format!(
                "failed to resolve GOLDENEYE_GRAMMAR_PACK_DIR {}: {error}. {}",
                candidate.display(),
                missing_pack_remediation()
            )
        })
    }

    #[cfg(feature = "compiled")]
    struct CompilerOptions {
        is_msvc: bool,
        supported_flags: Vec<OsString>,
    }

    #[cfg(feature = "compiled")]
    fn supported_compiler_options() -> Result<CompilerOptions, String> {
        let is_msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
        if !is_msvc {
            return Ok(CompilerOptions {
                is_msvc,
                supported_flags: Vec::new(),
            });
        }
        let build = cc::Build::new();
        let mut supported = Vec::new();
        for flag in ["/std:c11", "/utf-8", "/bigobj"] {
            if build
                .is_flag_supported(flag)
                .map_err(|error| format!("failed to probe MSVC flag {flag}: {error}"))?
            {
                supported.push(OsString::from(flag));
            }
        }
        Ok(CompilerOptions {
            is_msvc,
            supported_flags: supported,
        })
    }

    #[cfg(feature = "compiled")]
    fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<(), String> {
        if fs::read(path).is_ok_and(|existing| existing == bytes) {
            return Ok(());
        }
        fs::write(path, bytes)
            .map_err(|error| format!("failed to write wrapper {}: {error}", path.display()))
    }

    #[cfg(feature = "compiled")]
    fn compile_wrapper(
        wrapper: &WrapperSpec,
        wrapper_path: &Path,
        pack_root: &Path,
        compiler_options: &CompilerOptions,
    ) {
        let mut build = cc::Build::new();
        build
            .file(wrapper_path)
            .include(compiler_include_path(pack_root))
            .define("_DEFAULT_SOURCE", None)
            .warnings(false)
            .extra_warnings(false);
        for relative in wrapper.additional_include_paths() {
            build.include(compiler_include_path(&pack_root.join(relative)));
        }
        if !compiler_options.is_msvc {
            build.std("c11");
        }
        for flag in &compiler_options.supported_flags {
            build.flag(flag);
        }
        build.compile(&wrapper.archive_name());
    }
}
