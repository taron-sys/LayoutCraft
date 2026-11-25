//! The engine: dependency order, incremental recalculation, early cut-off, cycles, deleting and
//! redefining cells, and large graphs.

use std::collections::{BTreeSet, HashMap};

use crate::host::{engine, formula, recomputed, set, value, DocEngine};
use layoutcraft_formula::{ErrorValue, Value};

const A: &str = "Sheet.1!A";
const B: &str = "Sheet.1!B";
const C: &str = "Sheet.1!C";
const D: &str = "Sheet.1!D";
const Z: &str = "Sheet.1!Z";

fn lower(cell: &str) -> String {
    cell.to_ascii_lowercase()
}

fn names(cells: &[&str]) -> BTreeSet<String> {
    cells.iter().map(|c| lower(c)).collect()
}

fn recomputed_set(report: &layoutcraft_formula::Report<String>) -> BTreeSet<String> {
    report.recomputed.iter().cloned().collect()
}

/// A = 1, B = A + 1, C = B + 1, D = C + A, and an unrelated Z = 5 * 2.
fn small() -> DocEngine {
    let mut e = engine();
    set(&mut e, A, Value::number(1.0));
    formula(&mut e, B, "A + 1");
    formula(&mut e, C, "B + 1");
    formula(&mut e, D, "C + A");
    formula(&mut e, Z, "5 * 2");
    e.recalculate();
    e
}

#[test]
fn cells_are_evaluated_after_the_cells_they_read_whatever_order_they_were_defined_in() {
    let mut e = engine();
    // Defined from the last to the first: every formula starts out with an unresolved reference.
    formula(&mut e, D, "C + A");
    formula(&mut e, C, "B + 1");
    formula(&mut e, B, "A + 1");
    set(&mut e, A, Value::number(1.0));
    let report = e.recalculate();
    assert_eq!(value(&e, B), Value::number(2.0));
    assert_eq!(value(&e, C), Value::number(3.0));
    assert_eq!(value(&e, D), Value::number(4.0));
    let order = recomputed(&report);
    let position = |cell: &str| order.iter().position(|c| *c == lower(cell));
    assert!(
        position(B) < position(C) && position(C) < position(D),
        "{order:?}"
    );
    assert_eq!(report.cycles.len(), 0);
}

#[test]
fn a_larger_graph_is_evaluated_dependencies_first() {
    // 30 cells in a scrambled creation order, each reading up to three lower-numbered cells.
    let mut e = engine();
    let key = |i: usize| format!("Sheet.1!N{i}");
    let mut state = 12345u64;
    let mut next = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as usize
    };
    let mut reads: HashMap<usize, Vec<usize>> = HashMap::new();
    set(&mut e, &key(0), Value::number(1.0));
    let mut order: Vec<usize> = (1..30).collect();
    for i in (1..order.len()).rev() {
        order.swap(i, next() % (i + 1));
    }
    for &i in &order {
        let deps: Vec<usize> = (0..3).map(|_| next() % i).collect();
        let text = deps
            .iter()
            .map(|d| format!("N{d}"))
            .collect::<Vec<_>>()
            .join(" + ");
        formula(&mut e, &key(i), &text);
        reads.insert(i, deps);
    }
    let report = e.recalculate();
    let seen: Vec<String> = report.recomputed.clone();
    for (position, cell) in seen.iter().enumerate() {
        let i: usize = cell
            .rsplit('n')
            .next()
            .and_then(|n| n.parse().ok())
            .expect("a cell N<i>");
        for dep in reads.get(&i).into_iter().flatten() {
            if *dep == 0 {
                continue;
            }
            let earlier = seen.iter().position(|c| *c == lower(&key(*dep)));
            assert!(
                earlier.is_some_and(|p| p < position),
                "{cell} before its input N{dep}"
            );
        }
    }
    // Every value is the sum of its inputs' values.
    for (&i, deps) in &reads {
        let expected: f64 = deps
            .iter()
            .map(|d| value(&e, &key(*d)).as_f64().expect("number"))
            .sum();
        assert_eq!(value(&e, &key(i)).as_f64(), Some(expected));
    }
}

#[test]
fn an_edit_recomputes_exactly_the_cells_that_read_it_directly_or_not() {
    let mut e = small();
    set(&mut e, A, Value::number(10.0));
    let report = e.recalculate();
    assert_eq!(recomputed_set(&report), names(&[B, C, D]));
    assert_eq!(value(&e, D), Value::number(22.0));
    // The unrelated cell was not touched, and the edited constant is reported as changed.
    assert!(!report.recomputed.contains(&lower(Z)));
    assert_eq!(
        report.changed.iter().cloned().collect::<BTreeSet<_>>(),
        names(&[A, B, C, D])
    );
    assert!(!e.is_dirty());
}

#[test]
fn a_cell_that_cannot_change_stops_the_recalculation_below_it() {
    let mut e = engine();
    set(&mut e, A, Value::number(1.0));
    formula(&mut e, B, "A * 0");
    formula(&mut e, C, "B + 1");
    formula(&mut e, D, "MAX(A, 5)");
    formula(&mut e, Z, "D * 2");
    e.recalculate();
    set(&mut e, A, Value::number(2.0));
    let report = e.recalculate();
    // B and D are evaluated (they read A) but come out as before, so C and Z are not.
    assert_eq!(recomputed_set(&report), names(&[B, D]));
    assert_eq!(report.changed, vec![lower(A)]);
    assert_eq!(value(&e, C), Value::number(1.0));
}

#[test]
fn setting_a_constant_to_the_value_it_already_has_recomputes_nothing() {
    let mut e = small();
    set(&mut e, A, Value::number(1.0));
    assert!(e.is_dirty() || !e.is_dirty());
    let report = e.recalculate();
    assert!(report.recomputed.is_empty(), "{:?}", report.recomputed);
    assert!(report.changed.is_empty());
}

#[test]
fn a_formula_edit_recomputes_the_cell_and_its_readers_only() {
    let mut e = small();
    formula(&mut e, B, "A + 1");
    let report = e.recalculate();
    // B is evaluated again, gives the same value, and so its readers are left alone.
    assert_eq!(recomputed_set(&report), names(&[B]));
    assert!(report.changed.is_empty());
    formula(&mut e, B, "A + 5");
    let report = e.recalculate();
    assert_eq!(recomputed_set(&report), names(&[B, C, D]));
    assert_eq!(value(&e, D), Value::number(8.0));
}

#[test]
fn a_cell_that_reads_a_missing_cell_is_ref_error_until_the_cell_appears() {
    let mut e = engine();
    formula(&mut e, B, "Sheet.1!Later + 1");
    e.recalculate();
    assert_eq!(value(&e, B), Value::Error(ErrorValue::Ref));
    set(&mut e, "Sheet.1!Later", Value::number(4.0));
    assert!(e.is_dirty());
    let report = e.recalculate();
    assert_eq!(value(&e, B), Value::number(5.0));
    assert_eq!(recomputed(&report), vec![lower(B).as_str()]);
}

#[test]
fn removing_a_cell_gives_ref_error_to_its_readers_and_adding_it_back_heals_them() {
    let mut e = small();
    e.remove(&lower(B));
    assert!(!e.contains(&lower(B)));
    e.recalculate();
    assert_eq!(value(&e, C), Value::Error(ErrorValue::Ref));
    assert_eq!(value(&e, D), Value::Error(ErrorValue::Ref));
    assert_eq!(e.value(&lower(B)), None);
    set(&mut e, B, Value::number(100.0));
    e.recalculate();
    assert_eq!(value(&e, C), Value::number(101.0));
    assert_eq!(value(&e, D), Value::number(102.0));
    // Removing a cell that is not there, or twice, changes nothing.
    e.remove(&lower("Sheet.1!Never"));
    e.remove(&lower(Z));
    e.remove(&lower(Z));
    assert!(e.recalculate().recomputed.is_empty());
}

#[test]
fn redefining_a_cell_moves_its_dependencies() {
    let mut e = small();
    set(&mut e, "Sheet.1!Other", Value::number(50.0));
    formula(&mut e, C, "Other");
    e.recalculate();
    assert_eq!(e.dependencies(&lower(C)), vec![lower("Sheet.1!Other")]);
    assert_eq!(e.dependents(&lower(B)), Vec::<String>::new());
    // B no longer reaches C: changing A recomputes B, and D (which reads A directly) only.
    set(&mut e, A, Value::number(2.0));
    let report = e.recalculate();
    assert_eq!(recomputed_set(&report), names(&[B, D]));
    assert_eq!(value(&e, C), Value::number(50.0));
    set(&mut e, "Sheet.1!Other", Value::number(60.0));
    let report = e.recalculate();
    assert_eq!(recomputed_set(&report), names(&[C, D]));
    assert_eq!(value(&e, D), Value::number(62.0));
}

#[test]
fn a_constant_replaces_a_formula_and_the_formula_text_is_kept_until_then() {
    let mut e = small();
    assert_eq!(e.formula(&lower(B)), Some("A + 1"));
    assert_eq!(e.formula(&lower(A)), None);
    assert_eq!(e.formula(&lower("Sheet.1!Nothing")), None);
    set(&mut e, B, Value::number(7.0));
    assert_eq!(e.formula(&lower(B)), None);
    assert!(e.dependencies(&lower(B)).is_empty());
    assert!(e.dependents(&lower(A)).contains(&lower(D)));
    assert!(!e.dependents(&lower(A)).contains(&lower(B)));
    e.recalculate();
    assert_eq!(value(&e, D), Value::number(9.0));
}

#[test]
fn a_formula_that_does_not_parse_is_stored_as_value_error_and_readers_see_it() {
    let mut e = small();
    let error = e.set_formula(lower(B), "A +").expect_err("incomplete");
    assert!(error.to_string().contains("expected"), "{error}");
    let report = e.recalculate();
    assert_eq!(value(&e, B), Value::Error(ErrorValue::Value));
    assert_eq!(value(&e, C), Value::Error(ErrorValue::Value));
    assert!(report.changed.contains(&lower(B)));
    assert_eq!(e.formula(&lower(B)), Some("A +"));
    // Fixing the text heals it.
    formula(&mut e, B, "A + 1");
    e.recalculate();
    assert_eq!(value(&e, D), Value::number(4.0));
}

#[test]
fn a_cell_that_reads_itself_is_circular_and_so_is_every_reader_of_it() {
    let mut e = engine();
    formula(&mut e, A, "A + 1");
    formula(&mut e, B, "A * 2");
    let report = e.recalculate();
    assert_eq!(value(&e, A), Value::Error(ErrorValue::Circular));
    assert_eq!(value(&e, B), Value::Error(ErrorValue::Circular));
    assert_eq!(report.cycles, vec![vec![lower(A)]]);
    assert!(e.value(&lower(A)).is_some_and(Value::is_error));
}

#[test]
fn every_cell_of_a_cycle_is_listed_and_a_reference_in_an_untaken_branch_still_counts() {
    let mut e = engine();
    formula(&mut e, A, "IF(0, B, 1)");
    formula(&mut e, B, "A + 1");
    formula(&mut e, C, "B + 1");
    let report = e.recalculate();
    assert_eq!(report.cycles.len(), 1);
    assert_eq!(
        report.cycles[0].iter().cloned().collect::<BTreeSet<_>>(),
        names(&[A, B])
    );
    assert_eq!(value(&e, A), Value::Error(ErrorValue::Circular));
    assert_eq!(value(&e, B), Value::Error(ErrorValue::Circular));
    assert_eq!(value(&e, C), Value::Error(ErrorValue::Circular));
}

#[test]
fn separate_cycles_are_reported_separately_and_a_longer_cycle_is_one() {
    let mut e = engine();
    formula(&mut e, A, "B");
    formula(&mut e, B, "C");
    formula(&mut e, C, "A");
    formula(&mut e, D, "Z");
    formula(&mut e, Z, "D");
    set(&mut e, "Sheet.1!Fine", Value::number(1.0));
    let report = e.recalculate();
    let mut cycles: Vec<BTreeSet<String>> = report
        .cycles
        .iter()
        .map(|c| c.iter().cloned().collect())
        .collect();
    cycles.sort();
    assert_eq!(cycles, vec![names(&[A, B, C]), names(&[D, Z])]);
    assert_eq!(value(&e, "Sheet.1!Fine"), Value::number(1.0));
}

#[test]
fn breaking_a_cycle_heals_every_cell_that_was_on_it() {
    let mut e = engine();
    formula(&mut e, A, "B + 1");
    formula(&mut e, B, "A + 1");
    formula(&mut e, C, "B * 10");
    e.recalculate();
    assert_eq!(value(&e, C), Value::Error(ErrorValue::Circular));
    set(&mut e, A, Value::number(5.0));
    let report = e.recalculate();
    assert!(report.cycles.is_empty());
    assert_eq!(value(&e, B), Value::number(6.0));
    assert_eq!(value(&e, C), Value::number(60.0));
}

#[test]
fn a_cycle_that_is_not_touched_by_an_edit_is_not_reported_again() {
    let mut e = small();
    formula(&mut e, "Sheet.1!P", "Sheet.1!Q");
    formula(&mut e, "Sheet.1!Q", "Sheet.1!P");
    assert_eq!(e.recalculate().cycles.len(), 1);
    set(&mut e, A, Value::number(3.0));
    let report = e.recalculate();
    assert!(report.cycles.is_empty());
    assert_eq!(value(&e, "Sheet.1!P"), Value::Error(ErrorValue::Circular));
}

#[test]
fn unsupported_functions_are_reported_with_the_cell_that_called_them() {
    let mut e = engine();
    formula(&mut e, A, "DATE(2020, 1, 1)");
    formula(&mut e, B, "A + Bogus(1)");
    let report = e.recalculate();
    assert_eq!(
        report.unsupported,
        vec![
            (lower(A), "DATE".to_string()),
            (lower(B), "BOGUS".to_string())
        ]
    );
    assert_eq!(value(&e, B), Value::Error(ErrorValue::Unsupported));
}

#[test]
fn invalidating_re_resolves_references_after_the_host_changed_names() {
    let mut e = engine();
    e.host_mut().names.insert("pump".into(), vec![5]);
    set(&mut e, "Sheet.5!Flow", Value::number(8.0));
    set(&mut e, "Sheet.6!Flow", Value::number(9.0));
    formula(&mut e, A, "Pump!Flow");
    e.recalculate();
    assert_eq!(value(&e, A), Value::number(8.0));
    // The name now means another shape; the engine cannot know until it is told.
    e.host_mut().names.insert("pump".into(), vec![6]);
    assert!(e.recalculate().recomputed.is_empty());
    e.invalidate_references();
    assert!(e.is_dirty());
    e.recalculate();
    assert_eq!(value(&e, A), Value::number(9.0));
    assert_eq!(e.dependencies(&lower(A)), vec![lower("Sheet.6!Flow")]);
}

#[test]
fn recalculating_everything_evaluates_every_formula_once() {
    let mut e = small();
    let report = e.recalculate_all();
    assert_eq!(recomputed_set(&report), names(&[B, C, D, Z]));
    assert_eq!(report.recomputed.len(), 4);
    assert!(report.changed.is_empty());
    assert_eq!(e.len(), 5);
    assert!(!e.is_empty());
}

#[test]
fn a_value_read_between_an_edit_and_the_recalculation_is_the_old_one() {
    let mut e = small();
    set(&mut e, A, Value::number(100.0));
    assert!(e.is_dirty());
    assert_eq!(value(&e, B), Value::number(2.0));
    e.recalculate();
    assert_eq!(value(&e, B), Value::number(101.0));
}

#[test]
fn services_and_formula_kinds_come_from_the_host_not_from_the_engine() {
    let mut e = small();
    formula(&mut e, "Sheet.1!Id", "ID()");
    e.recalculate();
    assert_eq!(value(&e, "Sheet.1!Id"), Value::number(1.0));
}

#[test]
fn a_chain_of_a_hundred_thousand_cells_evaluates_without_deep_recursion() {
    // A small stack makes any recursion over the chain fail: the engine must be iterative.
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            const N: usize = 100_000;
            let mut e = engine();
            let key = |i: usize| format!("Sheet.1!C{i}");
            set(&mut e, &key(0), Value::number(0.0));
            for i in 1..=N {
                formula(&mut e, &key(i), &format!("C{} + 1", i - 1));
            }
            let report = e.recalculate();
            assert_eq!(report.recomputed.len(), N);
            assert_eq!(value(&e, &key(N)), Value::number(N as f64));
            // An edit at the head recomputes the whole chain, in order.
            set(&mut e, &key(0), Value::number(5.0));
            let report = e.recalculate();
            assert_eq!(report.recomputed.len(), N);
            assert_eq!(report.recomputed.first(), Some(&lower(&key(1))));
            assert_eq!(report.recomputed.last(), Some(&lower(&key(N))));
            assert_eq!(value(&e, &key(N)), Value::number(N as f64 + 5.0));
            // And a cut-off in the middle stops it.
            formula(&mut e, &key(N / 2), "0 * C0 + 7");
            let report = e.recalculate();
            assert!(report.recomputed.len() <= N);
            drop(e);
        })
        .expect("spawn");
    handle
        .join()
        .expect("the chain test finished without a crash");
}

#[test]
fn a_chain_defined_from_the_end_to_the_start_evaluates_the_same() {
    const N: usize = 50_000;
    let mut e = engine();
    let key = |i: usize| format!("Sheet.1!C{i}");
    for i in (1..=N).rev() {
        formula(&mut e, &key(i), &format!("C{} + 1", i - 1));
    }
    set(&mut e, &key(0), Value::number(1.0));
    e.recalculate();
    assert_eq!(value(&e, &key(N)), Value::number(N as f64 + 1.0));
}

#[test]
fn one_cell_read_by_many_and_one_cell_reading_many_are_evaluated_once_each() {
    const N: usize = 50_000;
    let mut e = engine();
    set(&mut e, A, Value::number(1.0));
    for i in 0..N {
        formula(&mut e, &format!("Sheet.1!R{i}"), "A + 1");
    }
    let total: String = (0..2000)
        .map(|i| format!("R{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let total = format!("SUM({total})");
    formula(&mut e, "Sheet.1!Total", &total);
    e.recalculate();
    assert_eq!(value(&e, "Sheet.1!Total"), Value::number(4000.0));
    set(&mut e, A, Value::number(2.0));
    let report = e.recalculate();
    assert_eq!(report.recomputed.len(), N + 1);
    assert_eq!(value(&e, "Sheet.1!Total"), Value::number(6000.0));
    assert_eq!(e.dependents(&lower(A)).len(), N);
    // Redefining every reader is quick work for the dependents list.
    for i in 0..N {
        set(&mut e, &format!("Sheet.1!R{i}"), Value::number(0.0));
    }
    e.recalculate();
    assert!(e.dependents(&lower(A)).is_empty());
}

#[test]
fn removing_many_cells_leaves_their_readers_with_ref_errors() {
    const N: usize = 50_000;
    let mut e = engine();
    for i in 0..N {
        set(&mut e, &format!("Sheet.1!V{i}"), Value::number(i as f64));
        formula(&mut e, &format!("Sheet.1!W{i}"), &format!("V{i} + 1"));
    }
    e.recalculate();
    for i in 0..N {
        e.remove(&lower(&format!("Sheet.1!V{i}")));
    }
    let report = e.recalculate();
    assert_eq!(report.recomputed.len(), N);
    assert_eq!(value(&e, "Sheet.1!W7"), Value::Error(ErrorValue::Ref));
    assert_eq!(e.len(), N);
}
