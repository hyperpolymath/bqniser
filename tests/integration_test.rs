// SPDX-License-Identifier: PMPL-1.0-or-later
// Copyright (c) 2026 Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//
// Integration tests for bqniser Phase 1.
//
// Tests cover:
// 1. Manifest parsing with [project], [[patterns]], and [bqn] sections
// 2. Manifest validation rules
// 3. Pattern analysis (parser -> ABI types)
// 4. BQN code generation (.bqn output)
// 5. FFI bridge generation (C header + Zig)
// 6. End-to-end generate pipeline
// 7. ABI primitive properties
// 8. Example manifest (examples/array-ops/)

use bqniser::abi::{
    pattern_from_entry, source_pattern_to_kind, ArrayPatternKind, BQNPrimitive,
};
use bqniser::codegen::bqn_gen::generate_bqn;
use bqniser::codegen::ffi_gen::generate_ffi;
use bqniser::codegen::parser::analyse_manifest;
use bqniser::manifest::{
    effective_name, load_manifest, validate, BqnConfig, Manifest, PatternEntry, SourcePattern,
};

/// Helper: build a minimal valid manifest in-memory.
fn sample_manifest() -> Manifest {
    Manifest {
        workload: Default::default(),
        data: Default::default(),
        options: Default::default(),
        project: bqniser::manifest::ProjectConfig {
            name: "test-project".to_string(),
            version: "0.1.0".to_string(),
            description: "Test project for bqniser".to_string(),
        },
        patterns: vec![
            PatternEntry {
                name: "sum-values".to_string(),
                source_pattern: SourcePattern::LoopSum,
                input_type: "f64".to_string(),
                output_type: "f64".to_string(),
            },
            PatternEntry {
                name: "double-each".to_string(),
                source_pattern: SourcePattern::MapTransform,
                input_type: "f64".to_string(),
                output_type: "f64".to_string(),
            },
            PatternEntry {
                name: "filter-positive".to_string(),
                source_pattern: SourcePattern::FilterPredicate,
                input_type: "f64".to_string(),
                output_type: "Vec<f64>".to_string(),
            },
            PatternEntry {
                name: "sort-asc".to_string(),
                source_pattern: SourcePattern::Sort,
                input_type: "i32".to_string(),
                output_type: "Vec<i32>".to_string(),
            },
            PatternEntry {
                name: "group-items".to_string(),
                source_pattern: SourcePattern::GroupBy,
                input_type: "f64".to_string(),
                output_type: "Vec<Vec<f64>>".to_string(),
            },
        ],
        bqn: BqnConfig {
            backend: "cbqn".to_string(),
            optimize: true,
        },
    }
}

// -----------------------------------------------------------------------
// Test 1: Manifest parsing from TOML string
// -----------------------------------------------------------------------

#[test]
fn test_manifest_parse_full() {
    let toml_str = r#"
[project]
name = "parse-test"
version = "1.0.0"
description = "Parsing test"

[[patterns]]
name = "my-sum"
source-pattern = "loop-sum"
input-type = "f64"
output-type = "f64"

[[patterns]]
name = "my-filter"
source-pattern = "filter-predicate"
input-type = "i32"
output-type = "Vec<i32>"

[bqn]
backend = "cbqn"
optimize = false
"#;

    let manifest: Manifest = toml::from_str(toml_str).expect("Failed to parse manifest");

    assert_eq!(manifest.project.name, "parse-test");
    assert_eq!(manifest.project.version, "1.0.0");
    assert_eq!(manifest.patterns.len(), 2);
    assert_eq!(manifest.patterns[0].name, "my-sum");
    assert_eq!(manifest.patterns[0].source_pattern, SourcePattern::LoopSum);
    assert_eq!(manifest.patterns[1].source_pattern, SourcePattern::FilterPredicate);
    assert_eq!(manifest.bqn.backend, "cbqn");
    assert!(!manifest.bqn.optimize);
}

// -----------------------------------------------------------------------
// Test 2: Manifest validation
// -----------------------------------------------------------------------

#[test]
fn test_manifest_validation_valid() {
    let m = sample_manifest();
    assert!(validate(&m).is_ok(), "Valid manifest should pass validation");
}

#[test]
fn test_manifest_validation_no_name() {
    let mut m = sample_manifest();
    m.project.name = String::new();
    m.workload.name = String::new();
    let result = validate(&m);
    assert!(result.is_err(), "Manifest without name should fail");
    assert!(
        result.unwrap_err().to_string().contains("name"),
        "Error should mention 'name'"
    );
}

#[test]
fn test_manifest_validation_bad_backend() {
    let mut m = sample_manifest();
    m.bqn.backend = "dzaima".to_string();
    let result = validate(&m);
    assert!(result.is_err(), "Unsupported backend should fail");
    assert!(
        result.unwrap_err().to_string().contains("dzaima"),
        "Error should mention the unsupported backend"
    );
}

#[test]
fn test_manifest_validation_empty_pattern_name() {
    let mut m = sample_manifest();
    m.patterns[0].name = String::new();
    let result = validate(&m);
    assert!(result.is_err(), "Pattern without name should fail");
}

// -----------------------------------------------------------------------
// Test 3: Pattern analysis (parser)
// -----------------------------------------------------------------------

#[test]
fn test_parser_analyse_manifest() {
    let m = sample_manifest();
    let program = analyse_manifest(&m).expect("Analysis should succeed");

    assert_eq!(program.project_name, "test-project");
    assert_eq!(program.patterns.len(), 5);
    assert!(program.optimised);

    // Check that each pattern has the right kind.
    assert_eq!(program.patterns[0].kind, ArrayPatternKind::LoopSum);
    assert_eq!(program.patterns[1].kind, ArrayPatternKind::MapTransform);
    assert_eq!(program.patterns[2].kind, ArrayPatternKind::FilterPredicate);
    assert_eq!(program.patterns[3].kind, ArrayPatternKind::Sort);
    assert_eq!(program.patterns[4].kind, ArrayPatternKind::GroupBy);

    // Check FFI declarations match.
    assert_eq!(program.ffi_declarations.len(), 5);
    assert_eq!(program.ffi_declarations[0].c_name, "bqniser_sum_values");
    assert_eq!(program.ffi_declarations[3].c_name, "bqniser_sort_asc");
}

// -----------------------------------------------------------------------
// Test 4: BQN code generation
// -----------------------------------------------------------------------

#[test]
fn test_bqn_generation() {
    let m = sample_manifest();
    let program = analyse_manifest(&m).unwrap();
    let bqn_source = generate_bqn(&program).expect("BQN generation should succeed");

    // The output should contain BQN primitives.
    assert!(
        bqn_source.contains("+\u{00b4}"),
        "BQN source should contain +´ (fold with addition)"
    );
    assert!(
        bqn_source.contains('\u{00a8}'),
        "BQN source should contain ¨ (each)"
    );
    assert!(
        bqn_source.contains('\u{234b}'),
        "BQN source should contain ⍋ (grade up)"
    );
    assert!(
        bqn_source.contains('\u{228f}'),
        "BQN source should contain ⊏ (select)"
    );
    assert!(
        bqn_source.contains('/'),
        "BQN source should contain / (replicate)"
    );

    // Should mention the project name.
    assert!(bqn_source.contains("test-project"));

    // Should have SPDX header.
    assert!(bqn_source.contains("PMPL-1.0-or-later"));

    // Should have an export namespace.
    assert!(bqn_source.contains("bqniser"));
}

// -----------------------------------------------------------------------
// Test 5: FFI bridge generation
// -----------------------------------------------------------------------

#[test]
fn test_ffi_generation() {
    let m = sample_manifest();
    let program = analyse_manifest(&m).unwrap();
    let (header, zig) = generate_ffi(&program).expect("FFI generation should succeed");

    // C header checks.
    assert!(header.contains("#ifndef BQNISER_FFI_H"));
    assert!(header.contains("bqniser_init"));
    assert!(header.contains("bqniser_cleanup"));
    assert!(header.contains("bqniser_sum_values"));
    assert!(header.contains("bqniser_sort_asc"));
    assert!(header.contains("const double*"));
    assert!(header.contains("size_t"));

    // Zig implementation checks.
    assert!(zig.contains("PMPL-1.0-or-later"));
    assert!(zig.contains("cbqn.h"));
    assert!(zig.contains("bqniser_init"));
    assert!(zig.contains("bqniser_cleanup"));
    assert!(zig.contains("bqn_eval"));
    assert!(zig.contains("bqniser_sum_values"));
}

// -----------------------------------------------------------------------
// Test 6: End-to-end generate pipeline (file I/O)
// -----------------------------------------------------------------------

#[test]
fn test_end_to_end_generate() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let manifest_path = dir.path().join("bqniser.toml");
    let output_dir = dir.path().join("output");

    // Write a manifest.
    let toml_content = r#"
[project]
name = "e2e-test"
version = "0.1.0"
description = "End-to-end test"

[[patterns]]
name = "total"
source-pattern = "loop-sum"
input-type = "f64"
output-type = "f64"

[[patterns]]
name = "reverse-sort"
source-pattern = "sort"
input-type = "i32"
output-type = "Vec<i32>"

[bqn]
backend = "cbqn"
optimize = true
"#;
    std::fs::write(&manifest_path, toml_content).unwrap();

    // Run the full pipeline.
    let result = bqniser::generate(
        manifest_path.to_str().unwrap(),
        output_dir.to_str().unwrap(),
    );
    assert!(result.is_ok(), "Generate pipeline should succeed: {:?}", result.err());

    // Check output files exist.
    assert!(output_dir.join("e2e-test.bqn").exists(), ".bqn file should exist");
    assert!(output_dir.join("bqniser_ffi.h").exists(), "C header should exist");
    assert!(output_dir.join("bqniser_ffi.zig").exists(), "Zig FFI should exist");

    // Check .bqn content.
    let bqn = std::fs::read_to_string(output_dir.join("e2e-test.bqn")).unwrap();
    assert!(bqn.contains("e2e-test"));
    assert!(bqn.contains("+\u{00b4}")); // +´

    // Check header content.
    let header = std::fs::read_to_string(output_dir.join("bqniser_ffi.h")).unwrap();
    assert!(header.contains("bqniser_total"));
    assert!(header.contains("bqniser_reverse_sort"));
}

// -----------------------------------------------------------------------
// Test 7: ABI primitive properties
// -----------------------------------------------------------------------

#[test]
fn test_bqn_primitive_properties() {
    // Glyphs.
    assert_eq!(BQNPrimitive::Join.glyph(), "\u{223e}");
    assert_eq!(BQNPrimitive::Reverse.glyph(), "\u{233d}");
    assert_eq!(BQNPrimitive::GradeUp.glyph(), "\u{234b}");
    assert_eq!(BQNPrimitive::Replicate.glyph(), "/");
    assert_eq!(BQNPrimitive::Select.glyph(), "\u{228f}");
    assert_eq!(BQNPrimitive::Reshape.glyph(), "\u{294a}");
    assert_eq!(BQNPrimitive::Fold.glyph(), "\u{00b4}");
    assert_eq!(BQNPrimitive::Scan.glyph(), "`");
    assert_eq!(BQNPrimitive::Each.glyph(), "\u{00a8}");
    assert_eq!(BQNPrimitive::Table.glyph(), "\u{231c}");

    // Labels.
    assert_eq!(BQNPrimitive::Join.label(), "Join");
    assert_eq!(BQNPrimitive::Fold.label(), "Fold");

    // Arities.
    assert_eq!(BQNPrimitive::Join.arity(), 2);
    assert_eq!(BQNPrimitive::Reverse.arity(), 1);
    assert_eq!(BQNPrimitive::GradeUp.arity(), 1);
    assert_eq!(BQNPrimitive::Replicate.arity(), 2);
    assert_eq!(BQNPrimitive::Select.arity(), 2);
    assert_eq!(BQNPrimitive::Reshape.arity(), 2);
    assert_eq!(BQNPrimitive::Fold.arity(), 1);
    assert_eq!(BQNPrimitive::Scan.arity(), 1);
    assert_eq!(BQNPrimitive::Each.arity(), 1);
    assert_eq!(BQNPrimitive::Table.arity(), 2);
}

// -----------------------------------------------------------------------
// Test 8: Pattern kind -> primitive mapping
// -----------------------------------------------------------------------

#[test]
fn test_pattern_kind_primitives() {
    let prims = ArrayPatternKind::LoopSum.primary_primitives();
    assert_eq!(prims, vec![BQNPrimitive::Fold]);

    let prims = ArrayPatternKind::MapTransform.primary_primitives();
    assert_eq!(prims, vec![BQNPrimitive::Each]);

    let prims = ArrayPatternKind::FilterPredicate.primary_primitives();
    assert_eq!(prims, vec![BQNPrimitive::Replicate]);

    let prims = ArrayPatternKind::Sort.primary_primitives();
    assert_eq!(prims, vec![BQNPrimitive::GradeUp, BQNPrimitive::Select]);

    let prims = ArrayPatternKind::GroupBy.primary_primitives();
    assert_eq!(prims, vec![BQNPrimitive::Replicate, BQNPrimitive::Each]);
}

// -----------------------------------------------------------------------
// Test 9: Source pattern to ABI kind conversion
// -----------------------------------------------------------------------

#[test]
fn test_source_pattern_to_kind() {
    assert_eq!(source_pattern_to_kind(&SourcePattern::LoopSum), ArrayPatternKind::LoopSum);
    assert_eq!(source_pattern_to_kind(&SourcePattern::MapTransform), ArrayPatternKind::MapTransform);
    assert_eq!(source_pattern_to_kind(&SourcePattern::FilterPredicate), ArrayPatternKind::FilterPredicate);
    assert_eq!(source_pattern_to_kind(&SourcePattern::Sort), ArrayPatternKind::Sort);
    assert_eq!(source_pattern_to_kind(&SourcePattern::GroupBy), ArrayPatternKind::GroupBy);
}

// -----------------------------------------------------------------------
// Test 10: Pattern from manifest entry
// -----------------------------------------------------------------------

#[test]
fn test_pattern_from_entry() {
    let entry = PatternEntry {
        name: "my-sort".to_string(),
        source_pattern: SourcePattern::Sort,
        input_type: "i64".to_string(),
        output_type: "Vec<i64>".to_string(),
    };
    let pat = pattern_from_entry(&entry);
    assert_eq!(pat.name, "my-sort");
    assert_eq!(pat.kind, ArrayPatternKind::Sort);
    assert_eq!(pat.primitives, vec![BQNPrimitive::GradeUp, BQNPrimitive::Select]);
    assert_eq!(pat.input_type, "i64");
    assert_eq!(pat.output_type, "Vec<i64>");
}

// -----------------------------------------------------------------------
// Test 11: Effective name (project vs workload fallback)
// -----------------------------------------------------------------------

#[test]
fn test_effective_name_prefers_project() {
    let mut m = sample_manifest();
    m.project.name = "project-name".to_string();
    m.workload.name = "workload-name".to_string();
    assert_eq!(effective_name(&m), "project-name");
}

#[test]
fn test_effective_name_falls_back_to_workload() {
    let mut m = sample_manifest();
    m.project.name = String::new();
    m.workload.name = "fallback-name".to_string();
    assert_eq!(effective_name(&m), "fallback-name");
}

// -----------------------------------------------------------------------
// Test 12: Example manifest loads and generates
// -----------------------------------------------------------------------

#[test]
fn test_example_array_ops_manifest() {
    let manifest_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/array-ops/bqniser.toml"
    );
    let m = load_manifest(manifest_path).expect("Example manifest should load");
    assert!(validate(&m).is_ok(), "Example manifest should be valid");
    assert_eq!(m.project.name, "array-ops-example");
    assert_eq!(m.patterns.len(), 5);

    // Generate into a temp dir.
    let dir = tempfile::tempdir().unwrap();
    let result = bqniser::generate(manifest_path, dir.path().to_str().unwrap());
    assert!(result.is_ok(), "Example generation should succeed: {:?}", result.err());
}
