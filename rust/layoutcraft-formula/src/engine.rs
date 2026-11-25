//! Dependency graph and recalculation.
//!
//! An [`Engine`] owns a set of cells, each either a constant or a formula. Formulas are parsed once;
//! their references are resolved through the host into edges of a dependency graph. After edits,
//! [`Engine::recalculate`] re-evaluates exactly the cells that can be affected, dependencies first,
//! and stops following an edge when a cell's new value equals its old one.
//!
//! Circular references: the public sources do not say how a cycle evaluates. Here every cell on a
//! cycle (including a cell that refers to itself) gets [`ErrorValue::Circular`], the cycle is
//! listed in the [`Report`], and cells that depend on it see that error like any other. Edges are
//! taken from the syntax tree, so a reference in a branch that is never taken still counts.

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;

use crate::ast::{CellRef, Expr};
use crate::env::{Env, FormulaKind, NoServices, Services};
use crate::error::{ErrorValue, ParseError};
use crate::eval::evaluate;
use crate::value::Value;

/// What the engine needs from the document model.
pub trait Host {
    /// Identifies a cell. Cheap to clone and compare.
    type Key: Clone + Eq + Hash + Debug;

    /// The cell `reference` names when written in the formula of `from`, if there is one. A key the
    /// engine was never given counts as a missing cell (`#REF!`).
    fn resolve(&self, from: &Self::Key, reference: &CellRef) -> Option<Self::Key>;

    /// Services for formulas of `from` (theme, palette, shape properties).
    fn services(&self, _from: &Self::Key) -> &dyn Services {
        &NoServices
    }

    /// `FORMULAEXISTS` support: `Some` overrides the engine's own answer, which is how a host
    /// reports inherited formulas.
    fn formula_kind(&self, _key: &Self::Key) -> Option<FormulaKind> {
        None
    }
}

struct Slot<K> {
    key: K,
    source: Option<String>,
    ast: Option<Expr>,
    value: Value,
    /// Slots this cell reads, deduplicated.
    deps: Vec<usize>,
}

/// What one [`Engine::recalculate`] did.
#[derive(Clone, Debug, PartialEq)]
pub struct Report<K> {
    /// Cells that were evaluated, in evaluation order (dependencies first).
    pub recomputed: Vec<K>,
    /// Cells whose value is different from before (including constants set since the last run).
    pub changed: Vec<K>,
    /// Circular references found; each is the set of cells on one cycle.
    pub cycles: Vec<Vec<K>>,
    /// Unsupported functions met while evaluating: the cell and the function name.
    pub unsupported: Vec<(K, String)>,
}

impl<K> Default for Report<K> {
    fn default() -> Self {
        Report {
            recomputed: Vec::new(),
            changed: Vec::new(),
            cycles: Vec::new(),
            unsupported: Vec::new(),
        }
    }
}

/// Cells, their dependency graph and their values.
pub struct Engine<H: Host> {
    host: H,
    index: HashMap<H::Key, usize>,
    /// Slot ids are never reused; a removed cell leaves `None`.
    slots: Vec<Option<Slot<H::Key>>>,
    /// `dependents[i]`: the slots that read slot `i`.
    dependents: Vec<Vec<usize>>,
    /// Slots whose formula must be (re-)evaluated, with a flag per slot to avoid repeats.
    dirty: Vec<usize>,
    dirty_flag: Vec<bool>,
    /// Slots that have an unresolved reference (retried when a cell appears).
    unresolved: Vec<usize>,
    retry_unresolved: bool,
    /// Constants changed since the last recalculation.
    pending_changed: Vec<usize>,
}

impl<H: Host> Engine<H> {
    pub fn new(host: H) -> Self {
        Engine {
            host,
            index: HashMap::new(),
            slots: Vec::new(),
            dependents: Vec::new(),
            dirty: Vec::new(),
            dirty_flag: Vec::new(),
            unresolved: Vec::new(),
            retry_unresolved: false,
            pending_changed: Vec::new(),
        }
    }

    pub fn host(&self) -> &H {
        &self.host
    }

    /// Mutable access to the host. If an edit changes what references resolve to, call
    /// [`Engine::invalidate_references`] afterwards.
    pub fn host_mut(&mut self) -> &mut H {
        &mut self.host
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn contains(&self, key: &H::Key) -> bool {
        self.index.contains_key(key)
    }

    /// The current value. Between an edit and [`Engine::recalculate`] it is stale.
    pub fn value(&self, key: &H::Key) -> Option<&Value> {
        let id = *self.index.get(key)?;
        self.slot(id).map(|slot| &slot.value)
    }

    /// The formula text, `None` for a constant or an unknown cell.
    pub fn formula(&self, key: &H::Key) -> Option<&str> {
        let id = *self.index.get(key)?;
        self.slot(id)?.source.as_deref()
    }

    /// The cells this cell reads, in order of first use in the formula.
    pub fn dependencies(&self, key: &H::Key) -> Vec<H::Key> {
        let Some(slot) = self.index.get(key).and_then(|&id| self.slot(id)) else {
            return Vec::new();
        };
        slot.deps
            .iter()
            .filter_map(|&dep| self.slot(dep))
            .map(|s| s.key.clone())
            .collect()
    }

    /// The cells that read this cell directly, in the order they were created.
    pub fn dependents(&self, key: &H::Key) -> Vec<H::Key> {
        let Some(&id) = self.index.get(key) else {
            return Vec::new();
        };
        let mut ids = self.dependents.get(id).cloned().unwrap_or_default();
        ids.sort_unstable();
        ids.into_iter()
            .filter_map(|dep| self.slot(dep))
            .map(|s| s.key.clone())
            .collect()
    }

    /// Whether an edit is waiting for [`Engine::recalculate`].
    pub fn is_dirty(&self) -> bool {
        !self.dirty.is_empty() || !self.pending_changed.is_empty() || self.retry_unresolved
    }

    fn slot(&self, id: usize) -> Option<&Slot<H::Key>> {
        self.slots.get(id).and_then(Option::as_ref)
    }

    fn slot_mut(&mut self, id: usize) -> Option<&mut Slot<H::Key>> {
        self.slots.get_mut(id).and_then(Option::as_mut)
    }

    /// The slot for `key`, created (as a constant `#N/A!`) if new.
    fn ensure(&mut self, key: &H::Key) -> usize {
        if let Some(&id) = self.index.get(key) {
            return id;
        }
        let id = self.slots.len();
        self.slots.push(Some(Slot {
            key: key.clone(),
            source: None,
            ast: None,
            value: Value::Error(ErrorValue::Na),
            deps: Vec::new(),
        }));
        self.dependents.push(Vec::new());
        self.dirty_flag.push(false);
        self.index.insert(key.clone(), id);
        // Formulas that pointed at this key before it existed can resolve now; retried once per
        // recalculation, not once per added cell.
        self.retry_unresolved = true;
        id
    }

    fn mark_dirty(&mut self, id: usize) {
        if let Some(flag) = self.dirty_flag.get_mut(id) {
            if !*flag {
                *flag = true;
                self.dirty.push(id);
            }
        }
    }

    /// Sets a cell to a constant.
    pub fn set_value(&mut self, key: H::Key, value: Value) {
        let id = self.ensure(&key);
        self.clear_formula(id);
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        if slot.value != value {
            slot.value = value;
            self.pending_changed.push(id);
        }
        for dependent in self.dependents.get(id).cloned().unwrap_or_default() {
            self.mark_dirty(dependent);
        }
    }

    /// Sets a cell's formula. A formula that does not parse is still stored: the cell becomes
    /// `#VALUE!` and the parse error is returned.
    pub fn set_formula(&mut self, key: H::Key, formula: &str) -> Result<(), ParseError> {
        let parsed = crate::parse(formula);
        let id = self.ensure(&key);
        let Some(slot) = self.slot_mut(id) else {
            return Ok(());
        };
        slot.source = Some(formula.to_string());
        let result = match parsed {
            Ok(ast) => {
                slot.ast = Some(ast);
                Ok(())
            }
            Err(error) => {
                slot.ast = None;
                slot.value = Value::Error(ErrorValue::Value);
                Err(error)
            }
        };
        self.relink(id);
        self.mark_dirty(id);
        result
    }

    /// Removes a cell. Formulas that read it see `#REF!` at the next recalculation.
    pub fn remove(&mut self, key: &H::Key) {
        let Some(id) = self.index.remove(key) else {
            return;
        };
        self.clear_formula(id);
        let readers = self
            .dependents
            .get_mut(id)
            .map(std::mem::take)
            .unwrap_or_default();
        if let Some(entry) = self.slots.get_mut(id) {
            *entry = None;
        }
        self.dirty.retain(|&d| d != id);
        if let Some(flag) = self.dirty_flag.get_mut(id) {
            *flag = false;
        }
        self.pending_changed.retain(|&d| d != id);
        self.unresolved.retain(|&d| d != id);
        for reader in readers {
            self.relink(reader);
            self.mark_dirty(reader);
        }
    }

    /// Re-resolves every reference and re-evaluates every formula at the next recalculation. Call
    /// it after the host changed what names and IDs mean.
    pub fn invalidate_references(&mut self) {
        for id in 0..self.slots.len() {
            if self.slot(id).is_some_and(|s| s.ast.is_some()) {
                self.relink(id);
                self.mark_dirty(id);
            }
        }
    }

    /// Marks every formula for evaluation at the next recalculation.
    pub fn invalidate_all(&mut self) {
        for id in 0..self.slots.len() {
            if self.slot(id).is_some_and(|s| s.ast.is_some()) {
                self.mark_dirty(id);
            }
        }
    }

    fn clear_formula(&mut self, id: usize) {
        if let Some(slot) = self.slot_mut(id) {
            slot.source = None;
            slot.ast = None;
        }
        self.set_deps(id, Vec::new(), false);
    }

    /// Recomputes the edges of one formula from its syntax tree.
    fn relink(&mut self, id: usize) {
        let Some(slot) = self.slot(id) else {
            return;
        };
        let mut deps: Vec<usize> = Vec::new();
        let mut unresolved = false;
        if let Some(ast) = &slot.ast {
            for reference in ast.references() {
                let target = self
                    .host
                    .resolve(&slot.key, reference)
                    .and_then(|key| self.index.get(&key).copied());
                match target {
                    Some(dep) => {
                        if !deps.contains(&dep) {
                            deps.push(dep);
                        }
                    }
                    None => unresolved = true,
                }
            }
        }
        self.set_deps(id, deps, unresolved);
    }

    fn set_deps(&mut self, id: usize, deps: Vec<usize>, unresolved: bool) {
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        let old = std::mem::replace(&mut slot.deps, deps.clone());
        for dep in old {
            if let Some(list) = self.dependents.get_mut(dep) {
                list.retain(|&reader| reader != id);
            }
        }
        for dep in deps {
            if let Some(list) = self.dependents.get_mut(dep) {
                list.push(id);
            }
        }
        self.unresolved.retain(|&waiting| waiting != id);
        if unresolved {
            self.unresolved.push(id);
        }
    }

    /// Every slot that can be affected by the pending edits: the dirty ones and all their
    /// transitive readers, in slot order.
    fn affected(&self) -> Vec<usize> {
        let mut seen = vec![false; self.slots.len()];
        let mut stack: Vec<usize> = Vec::new();
        for &id in &self.dirty {
            if let Some(flag) = seen.get_mut(id) {
                if !*flag && self.slot(id).is_some() {
                    *flag = true;
                    stack.push(id);
                }
            }
        }
        while let Some(id) = stack.pop() {
            for &reader in self.dependents.get(id).map_or(&[][..], Vec::as_slice) {
                if let Some(flag) = seen.get_mut(reader) {
                    if !*flag {
                        *flag = true;
                        stack.push(reader);
                    }
                }
            }
        }
        seen.iter()
            .enumerate()
            .filter(|(_, &yes)| yes)
            .map(|(id, _)| id)
            .collect()
    }

    /// Evaluates the cells the pending edits can affect and returns what happened.
    pub fn recalculate(&mut self) -> Report<H::Key> {
        let mut report = Report::default();
        if std::mem::take(&mut self.retry_unresolved) {
            for waiting in std::mem::take(&mut self.unresolved) {
                self.relink(waiting);
                self.mark_dirty(waiting);
            }
        }
        let affected = self.affected();
        let mut needs = vec![false; self.slots.len()];
        for &id in &self.dirty {
            if let Some(flag) = needs.get_mut(id) {
                *flag = true;
            }
        }
        for id in std::mem::take(&mut self.pending_changed) {
            if let Some(slot) = self.slot(id) {
                report.changed.push(slot.key.clone());
            }
        }
        for component in self.components(&affected) {
            self.process(&component, &mut needs, &mut report);
        }
        for id in std::mem::take(&mut self.dirty) {
            if let Some(flag) = self.dirty_flag.get_mut(id) {
                *flag = false;
            }
        }
        report
    }

    /// Re-evaluates every formula.
    pub fn recalculate_all(&mut self) -> Report<H::Key> {
        self.invalidate_all();
        self.recalculate()
    }

    /// Evaluates one strongly connected component (a single cell, or a cycle).
    fn process(&mut self, component: &[usize], needs: &mut [bool], report: &mut Report<H::Key>) {
        let is_cycle = component.len() > 1
            || component
                .first()
                .is_some_and(|&id| self.slot(id).is_some_and(|s| s.deps.contains(&id)));
        if is_cycle {
            let mut keys = Vec::new();
            for &id in component {
                if let Some(slot) = self.slots.get_mut(id).and_then(Option::as_mut) {
                    keys.push(slot.key.clone());
                    let circular = Value::Error(ErrorValue::Circular);
                    if slot.value != circular {
                        slot.value = circular;
                        report.changed.push(slot.key.clone());
                        Self::wake(&self.dependents, id, needs);
                    }
                }
            }
            report.cycles.push(keys);
            return;
        }
        let Some(&id) = component.first() else {
            return;
        };
        if !needs.get(id).copied().unwrap_or(false) {
            return;
        }
        let Some(slot) = self.slot(id) else {
            return;
        };
        let Some(ast) = &slot.ast else {
            // A formula that did not parse keeps its error value; readers still need a look.
            Self::wake(&self.dependents, id, needs);
            return;
        };
        let env = EngineEnv {
            engine: self,
            from: id,
        };
        let evaluation = evaluate(ast, &env);
        let key = slot.key.clone();
        report.recomputed.push(key.clone());
        for name in evaluation.unsupported {
            report.unsupported.push((key.clone(), name));
        }
        if let Some(slot) = self.slots.get_mut(id).and_then(Option::as_mut) {
            if slot.value != evaluation.value {
                slot.value = evaluation.value;
                report.changed.push(key);
                Self::wake(&self.dependents, id, needs);
            }
        }
    }

    fn wake(dependents: &[Vec<usize>], id: usize, needs: &mut [bool]) {
        for &reader in dependents.get(id).map_or(&[][..], Vec::as_slice) {
            if let Some(flag) = needs.get_mut(reader) {
                *flag = true;
            }
        }
    }

    /// Strongly connected components of the graph restricted to `affected`, dependencies first
    /// (iterative Tarjan, so a long chain cannot overflow the stack).
    fn components(&self, affected: &[usize]) -> Vec<Vec<usize>> {
        let n = self.slots.len();
        let mut inside = vec![false; n];
        for &id in affected {
            if let Some(flag) = inside.get_mut(id) {
                *flag = true;
            }
        }
        let mut order: Vec<Option<usize>> = vec![None; n];
        let mut low = vec![0usize; n];
        let mut on_stack = vec![false; n];
        let mut stack: Vec<usize> = Vec::new();
        let mut counter = 0usize;
        let mut result: Vec<Vec<usize>> = Vec::new();
        // Every index below is a slot id, and slot ids are always < n.
        for &root in affected {
            if order[root].is_some() {
                continue;
            }
            // Each frame: (node, position in its dependency list).
            let mut frames: Vec<(usize, usize)> = vec![(root, 0)];
            order[root] = Some(counter);
            low[root] = counter;
            counter += 1;
            stack.push(root);
            on_stack[root] = true;
            while let Some(&(node, next)) = frames.last() {
                let deps: &[usize] = self.slot(node).map_or(&[], |s| s.deps.as_slice());
                if let Some(&dep) = deps.get(next) {
                    if let Some(frame) = frames.last_mut() {
                        frame.1 += 1;
                    }
                    if !inside[dep] {
                        continue;
                    }
                    match order[dep] {
                        None => {
                            order[dep] = Some(counter);
                            low[dep] = counter;
                            counter += 1;
                            stack.push(dep);
                            on_stack[dep] = true;
                            frames.push((dep, 0));
                        }
                        Some(seen) if on_stack[dep] => low[node] = low[node].min(seen),
                        Some(_) => {}
                    }
                    continue;
                }
                frames.pop();
                if let Some(&(parent, _)) = frames.last() {
                    low[parent] = low[parent].min(low[node]);
                }
                if order[node] == Some(low[node]) {
                    let mut component = Vec::new();
                    while let Some(member) = stack.pop() {
                        on_stack[member] = false;
                        component.push(member);
                        if member == node {
                            break;
                        }
                    }
                    component.sort_unstable();
                    result.push(component);
                }
            }
        }
        result
    }
}

/// The environment one formula sees while the engine evaluates it.
struct EngineEnv<'a, H: Host> {
    engine: &'a Engine<H>,
    from: usize,
}

impl<H: Host> EngineEnv<'_, H> {
    fn source_key(&self) -> Option<&H::Key> {
        self.engine.slot(self.from).map(|slot| &slot.key)
    }

    fn target(&self, reference: &CellRef) -> Option<usize> {
        let key = self.engine.host.resolve(self.source_key()?, reference)?;
        self.engine.index.get(&key).copied()
    }
}

impl<H: Host> Env for EngineEnv<'_, H> {
    fn cell(&self, reference: &CellRef) -> Result<Value, ErrorValue> {
        let id = self.target(reference).ok_or(ErrorValue::Ref)?;
        self.engine
            .slot(id)
            .map(|slot| slot.value.clone())
            .ok_or(ErrorValue::Ref)
    }

    fn formula_kind(&self, reference: &CellRef) -> Result<FormulaKind, ErrorValue> {
        let id = self.target(reference).ok_or(ErrorValue::Ref)?;
        let slot = self.engine.slot(id).ok_or(ErrorValue::Ref)?;
        if let Some(kind) = self.engine.host.formula_kind(&slot.key) {
            return Ok(kind);
        }
        Ok(if slot.source.is_some() {
            FormulaKind::Local
        } else {
            FormulaKind::Absent
        })
    }

    fn services(&self) -> &dyn Services {
        match self.source_key() {
            Some(key) => self.engine.host.services(key),
            None => &NoServices,
        }
    }
}
