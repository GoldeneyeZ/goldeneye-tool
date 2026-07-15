mod common;

use common::Fixture;
use goldeneye_query::{QueryError, TraceDirection, TracePathRequest};

#[test]
fn trace_path_walks_inbound_and_outbound_calls_with_stable_hops() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();

    let outbound = engine
        .trace_path(&TracePathRequest::new(
            fixture.project.clone(),
            "demo.src.lib.Alpha",
            TraceDirection::Outbound,
        ))
        .expect("outbound trace");
    assert_eq!(outbound.paths.len(), 1);
    assert_eq!(
        outbound.paths[0].source_qualified_name,
        "demo.src.lib.Alpha"
    );
    assert_eq!(outbound.paths[0].target_qualified_name, "demo.src.lib.beta");
    assert_eq!(outbound.paths[0].hop, 1);

    let inbound = engine
        .trace_path(&TracePathRequest::new(
            fixture.project.clone(),
            "demo.src.lib.Alpha",
            TraceDirection::Inbound,
        ))
        .expect("inbound trace");
    assert_eq!(
        inbound
            .paths
            .iter()
            .map(|path| path.source_qualified_name.as_str())
            .collect::<Vec<_>>(),
        vec!["demo.src.lib.beta", "demo.src.lib.main"]
    );
}

#[test]
fn trace_depth_cycles_and_limit_are_bounded() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();

    let mut request = TracePathRequest::new(
        fixture.project.clone(),
        "demo.src.lib.main",
        TraceDirection::Outbound,
    );
    request.depth = 5;
    let cycle = engine.trace_path(&request).expect("cycle-safe trace");
    assert_eq!(
        cycle
            .paths
            .iter()
            .map(|path| (path.related_qualified_name.as_str(), path.hop))
            .collect::<Vec<_>>(),
        vec![("demo.src.lib.Alpha", 1), ("demo.src.lib.beta", 2)]
    );
    assert!(!cycle.truncated);

    request.limit = 1;
    let limited = engine.trace_path(&request).expect("limited trace");
    assert_eq!(limited.paths.len(), 1);
    assert!(limited.truncated);
}

#[test]
fn trace_call_path_alias_is_identical_and_short_name_ambiguity_is_typed() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let request = TracePathRequest::new(fixture.project.clone(), "Alpha", TraceDirection::Outbound);
    assert_eq!(
        engine.trace_path(&request).expect("primary trace"),
        engine
            .trace_call_path(&request)
            .expect("compatibility alias trace")
    );

    let ambiguous = TracePathRequest::new(fixture.project.clone(), "run", TraceDirection::Outbound);
    match engine.trace_path(&ambiguous) {
        Err(QueryError::AmbiguousSymbol { candidates, .. }) => assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.qualified_name.as_str())
                .collect::<Vec<_>>(),
            vec!["demo.src.lib.Café.run", "demo.src.lib.run"]
        ),
        other => panic!("expected typed ambiguity, got {other:?}"),
    }
}

#[test]
fn trace_rejects_out_of_contract_depth_and_limits() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let mut request =
        TracePathRequest::new(fixture.project.clone(), "Alpha", TraceDirection::Outbound);
    request.depth = 0;
    assert!(matches!(
        engine.trace_path(&request),
        Err(QueryError::InvalidTraceDepth { .. })
    ));
    request.depth = 1;
    request.limit = 0;
    assert!(matches!(
        engine.trace_path(&request),
        Err(QueryError::InvalidTraceLimit { .. })
    ));
}
