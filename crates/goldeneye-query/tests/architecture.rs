mod common;

use common::Fixture;
use goldeneye_query::ArchitectureRequest;

#[test]
fn architecture_is_a_deterministic_module_type_entrypoint_and_edge_summary() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let request = ArchitectureRequest::new(fixture.project.clone());

    let architecture = engine.get_architecture(&request).expect("architecture");
    assert_eq!(
        architecture,
        engine.get_architecture(&request).expect("repeat")
    );
    assert_eq!(
        (architecture.total_nodes, architecture.total_edges),
        (7, 10)
    );
    assert_eq!(
        architecture
            .languages
            .iter()
            .map(|entry| (entry.name.as_str(), entry.count))
            .collect::<Vec<_>>(),
        vec![("rust", 1)]
    );
    assert_eq!(architecture.modules.len(), 1);
    assert_eq!(architecture.modules[0].qualified_name, "demo.src.lib");
    assert_eq!(architecture.modules[0].defined_symbols, 5);
    assert_eq!(
        architecture
            .types
            .iter()
            .map(|node| node.qualified_name.as_str())
            .collect::<Vec<_>>(),
        vec!["demo.src.lib.Café"]
    );
    assert_eq!(
        architecture
            .entry_points
            .iter()
            .map(|node| node.qualified_name.as_str())
            .collect::<Vec<_>>(),
        vec!["demo.src.lib.main"]
    );
    assert_eq!(
        architecture
            .edge_types
            .iter()
            .map(|entry| (entry.name.as_str(), entry.count))
            .collect::<Vec<_>>(),
        vec![("CALLS", 4), ("DEFINES", 6)]
    );
}
