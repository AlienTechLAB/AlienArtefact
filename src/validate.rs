//! Whole-diagram rule check: every rule of docs/language-spec.md that can be decided from the
//! diagram, not only the ones enforced while drawing.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::model::{Diagram, Edge, End, Id, LinkKind, MemberKind, PointerKind, RelationKind, ShapeKind, Visibility};

/// One broken rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    /// Rule id from the spec, e.g. `P3`.
    pub rule: &'static str,
    pub message: String,
    /// Objects and lines involved (the first one is shown when the issue is clicked).
    pub items: Vec<Id>,
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Strongly connected components with a cycle (more than one node, or a node with an edge to
/// itself). Tarjan's algorithm.
fn cycles(edges: &[(Id, Id)]) -> Vec<Vec<Id>> {
    let mut graph: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
    for &(a, b) in edges {
        graph.entry(a).or_default().push(b);
        graph.entry(b).or_default();
    }
    struct State {
        index: usize,
        indices: HashMap<Id, usize>,
        low: HashMap<Id, usize>,
        stack: Vec<Id>,
        on_stack: BTreeSet<Id>,
        out: Vec<Vec<Id>>,
    }
    fn visit(v: Id, g: &BTreeMap<Id, Vec<Id>>, st: &mut State) {
        st.indices.insert(v, st.index);
        st.low.insert(v, st.index);
        st.index += 1;
        st.stack.push(v);
        st.on_stack.insert(v);
        for &w in &g[&v] {
            if !st.indices.contains_key(&w) {
                visit(w, g, st);
                let lw = st.low[&w];
                let lv = st.low.get_mut(&v).unwrap();
                *lv = (*lv).min(lw);
            } else if st.on_stack.contains(&w) {
                let iw = st.indices[&w];
                let lv = st.low.get_mut(&v).unwrap();
                *lv = (*lv).min(iw);
            }
        }
        if st.low[&v] == st.indices[&v] {
            let mut comp = Vec::new();
            while let Some(w) = st.stack.pop() {
                st.on_stack.remove(&w);
                comp.push(w);
                if w == v {
                    break;
                }
            }
            let self_loop = g[&v].contains(&v);
            if comp.len() > 1 || self_loop {
                comp.reverse();
                st.out.push(comp);
            }
        }
    }
    let mut st = State {
        index: 0,
        indices: HashMap::new(),
        low: HashMap::new(),
        stack: Vec::new(),
        on_stack: BTreeSet::new(),
        out: Vec::new(),
    };
    let keys: Vec<Id> = graph.keys().copied().collect();
    for v in keys {
        if !st.indices.contains_key(&v) {
            visit(v, &graph, &mut st);
        }
    }
    st.out
}

/// Rule id for a line that may not connect its ends.
fn link_rule(kind: LinkKind) -> &'static str {
    match kind {
        LinkKind::Pointer(_) => "P12",
        LinkKind::Relation(RelationKind::Implements) => "R1",
        LinkKind::Relation(RelationKind::Inherits) => "R2",
        LinkKind::Relation(RelationKind::Related) => "R9",
        LinkKind::Relation(RelationKind::Uses) => "U1",
    }
}

pub fn validate(d: &Diagram) -> Vec<Issue> {
    let mut out = Vec::new();
    let mut issue = |rule: &'static str, message: String, items: Vec<Id>| out.push(Issue { rule, message, items });
    let name_of = |id: Id| d.node(id).map_or_else(|| "?".to_owned(), |n| d.display_text(n));
    let node_end = |e: End| e.node().and_then(|id| d.node(id));
    let is_element_pointer =
        |e: &Edge| e.pointer.is_some() && node_end(e.from).is_some_and(|n| n.kind.is_collection());

    // ---------------------------------------------------------------- objects
    let mut type_names: BTreeMap<(Option<Id>, String), Vec<Id>> = BTreeMap::new();
    let mut storage_names: BTreeMap<String, Vec<Id>> = BTreeMap::new();
    for n in &d.nodes {
        if !n.kind.is_language() {
            issue("D1", format!("\"{}\" is a drawing shape ({}), not part of the language.", n.text, n.kind.name()), vec![n.id]);
            continue;
        }
        let name = n.text.trim();
        if n.kind.is_named() && name.is_empty() {
            let rule = if n.kind.is_storage() { "F1" } else { "O1" };
            issue(rule, format!("A {} has no name.", n.kind.name().to_lowercase()), vec![n.id]);
        }
        if n.kind.is_object() && !name.is_empty() && !is_identifier(name) {
            issue("O3", format!("\"{name}\" is not a valid type name (letters, digits and _ only)."), vec![n.id]);
        }
        if n.kind.is_object() || n.kind == ShapeKind::External || n.kind == ShapeKind::Package {
            if !name.is_empty() {
                type_names.entry((d.package_of(n.id), name.to_owned())).or_default().push(n.id);
            }
        }
        if n.kind.is_storage() && !name.is_empty() {
            storage_names.entry(name.to_owned()).or_default().push(n.id);
        }

        // members
        let mut fields: BTreeMap<&str, usize> = BTreeMap::new();
        let mut variants: BTreeMap<&str, usize> = BTreeMap::new();
        for m in &n.members {
            if !is_identifier(m.name.trim()) {
                issue("M1", format!("\"{}\" has a member named \"{}\", which is not a valid identifier.", n.text, m.name), vec![n.id]);
            }
            match m.kind {
                MemberKind::Field => {
                    *fields.entry(m.name.trim()).or_default() += 1;
                    if m.ty.trim().is_empty() {
                        issue("M7", format!("Field \"{}.{}\" has no type.", n.text, m.name), vec![n.id]);
                    }
                    if n.kind == ShapeKind::Trait {
                        issue("M5", format!("Trait \"{}\" has a field \"{}\"; traits have methods only.", n.text, m.name), vec![n.id]);
                    }
                }
                MemberKind::Variant => *variants.entry(m.name.trim()).or_default() += 1,
                MemberKind::Method => {}
            }
            if n.kind == ShapeKind::Trait && m.visibility != Visibility::Public {
                issue("M6", format!("Trait method \"{}.{}\" is not public.", n.text, m.name), vec![n.id]);
            }
        }
        for (f, count) in &fields {
            if *count > 1 {
                issue("M2", format!("\"{}\" has {count} fields named \"{f}\".", n.text), vec![n.id]);
            }
        }
        for (v, count) in &variants {
            if *count > 1 {
                issue("E2", format!("Enum \"{}\" has {count} variants named \"{v}\".", n.text), vec![n.id]);
            }
        }
        if n.kind == ShapeKind::Enum && variants.is_empty() {
            issue("E1", format!("Enum \"{}\" has no variants.", n.text), vec![n.id]);
        }

        // collections
        if n.kind.is_collection() {
            if d.element_pointer(n.id, None).is_none() && n.text.trim().is_empty() {
                issue("C3", format!("The {} has no element type.", n.kind.name().to_lowercase()), vec![n.id]);
            }
            if n.kind == ShapeKind::Dictionary && n.key.trim().is_empty() {
                issue("C4", "A dictionary has no key type.".to_owned(), vec![n.id]);
            }
        }
    }
    for ((_, name), ids) in &type_names {
        if ids.len() > 1 {
            issue("O2", format!("{} objects are named \"{name}\" in the same package.", ids.len()), ids.clone());
        }
    }
    for (name, ids) in &storage_names {
        if ids.len() > 1 {
            issue("F2", format!("{} files or directories are named \"{name}\".", ids.len()), ids.clone());
        }
    }

    // ---------------------------------------------------------------- lines
    let mut labels: BTreeMap<(Id, String), Vec<Id>> = BTreeMap::new();
    let mut shared_targets: BTreeSet<Id> = BTreeSet::new();
    let mut shared_graph = Vec::new();
    let mut inherit_graph = Vec::new();
    let mut pairs: BTreeMap<(Id, Id, &'static str), Vec<Id>> = BTreeMap::new();
    for e in &d.edges {
        let Some(kind) = e.link() else {
            issue("D1", "A plain connector is not part of the language; give it a pointer or relation type.".into(), vec![e.id]);
            continue;
        };
        if let Some(message) = d.link_error_for(Some(e.id), kind, e.from, e.to) {
            issue(link_rule(kind), message.replace("\n\n", " "), vec![e.id]);
        }
        let ends = (e.from.node(), e.to.node());
        match kind {
            LinkKind::Pointer(p) => {
                if ends.0.is_none() || ends.1.is_none() {
                    issue("P2", format!("A {} has an end that is not attached to an object.", p.name().to_lowercase()), vec![e.id]);
                }
                if let (Some(a), Some(b)) = ends {
                    if p == PointerKind::Shared {
                        shared_graph.push((a, b));
                        shared_targets.insert(b);
                    }
                    if a == b && e.required && matches!(p, PointerKind::Unique | PointerKind::Shared) {
                        issue("P6", format!("\"{}\" owns a never-null value of its own type, which would be infinitely large.", name_of(a)), vec![e.id]);
                    }
                    if p == PointerKind::Reference && a == b {
                        issue("P7", format!("\"{}\" holds a reference to its own type; use Raw or Weak.", name_of(a)), vec![e.id]);
                    }
                }
                if !is_element_pointer(e) {
                    if e.label.trim().is_empty() {
                        issue("P3", format!("A {} from \"{}\" has no label (member name).", p.name().to_lowercase(), ends.0.map_or("?".into(), name_of)), vec![e.id]);
                    } else if let Some(a) = ends.0 {
                        labels.entry((a, e.label.trim().to_owned())).or_default().push(e.id);
                    }
                }
            }
            LinkKind::Relation(r) => {
                if let (Some(a), Some(b)) = ends {
                    if r == RelationKind::Inherits {
                        inherit_graph.push((a, b));
                    }
                    let tag = match r {
                        RelationKind::Implements => "implements",
                        RelationKind::Inherits => "inherits from",
                        RelationKind::Uses => "uses",
                        RelationKind::Related => "",
                    };
                    if !tag.is_empty() {
                        pairs.entry((a, b, tag)).or_default().push(e.id);
                    }
                }
            }
        }
    }
    for ((owner, label), ids) in &labels {
        let field_clash = d.node(*owner).is_some_and(|n| {
            n.members.iter().any(|m| m.kind == MemberKind::Field && m.name.trim() == label)
        });
        if ids.len() > 1 {
            issue("P4", format!("\"{}\" has {} pointers labeled \"{label}\".", name_of(*owner), ids.len()), ids.clone());
        }
        if field_clash {
            issue("M4", format!("\"{}\" has both a field and a pointer named \"{label}\".", name_of(*owner)), ids.clone());
        }
    }
    for e in &d.edges {
        if e.pointer == Some(PointerKind::Weak) {
            if let Some(b) = e.to.node() {
                if !shared_targets.contains(&b) {
                    issue("P8", format!("\"{}\" is observed by a weak pointer but owned by no shared pointer.", name_of(b)), vec![e.id, b]);
                }
            }
        }
    }
    for cycle in cycles(&shared_graph) {
        let names: Vec<String> = cycle.iter().map(|id| name_of(*id)).collect();
        issue("P9", format!("Shared pointers form a cycle that never frees itself: {}. Make one of them Weak.", names.join(" \u{2192} ")), cycle);
    }
    for cycle in cycles(&inherit_graph) {
        let names: Vec<String> = cycle.iter().map(|id| name_of(*id)).collect();
        issue("R5", format!("Inheritance cycle: {}.", names.join(" \u{2192} ")), cycle);
    }
    for ((a, b, tag), ids) in &pairs {
        if ids.len() > 1 {
            issue("R6", format!("\"{}\" {tag} \"{}\" {} times.", name_of(*a), name_of(*b), ids.len()), ids.clone());
        }
    }

    // ---------------------------------------------------------------- packages
    let package_uses: BTreeSet<(Id, Id)> = d
        .edges
        .iter()
        .filter(|e| e.relation == Some(RelationKind::Uses))
        .filter_map(|e| Some((e.from.node()?, e.to.node()?)))
        .filter(|(a, b)| {
            d.node(*a).is_some_and(|n| n.kind == ShapeKind::Package)
                && d.node(*b).is_some_and(|n| n.kind == ShapeKind::Package)
        })
        .collect();
    for e in &d.edges {
        let Some(kind) = e.link() else { continue };
        if matches!(kind, LinkKind::Relation(RelationKind::Related)) {
            continue; // Related is not a code dependency.
        }
        let (Some(a), Some(b)) = (e.from.node(), e.to.node()) else { continue };
        if d.node(a).is_some_and(|n| n.kind == ShapeKind::Package) {
            continue; // the declaration itself
        }
        let (ca, cb) = (d.package_chain(a), d.package_chain(b));
        let (Some(&pa), Some(&pb)) = (ca.first(), cb.first()) else { continue };
        // Same package, or one package encloses the other: no declaration needed.
        if pa == pb || ca.contains(&pb) || cb.contains(&pa) {
            continue;
        }
        let declared = ca.iter().any(|x| cb.iter().any(|y| package_uses.contains(&(*x, *y))));
        if !declared {
            issue(
                "L1",
                format!(
                    "\"{}\" (package {}) {} \"{}\" (package {}), but package {} does not use package {}.",
                    name_of(a),
                    name_of(pa),
                    match kind {
                        LinkKind::Pointer(_) => "points to",
                        LinkKind::Relation(RelationKind::Uses) => "uses",
                        LinkKind::Relation(RelationKind::Implements) => "implements",
                        _ => "inherits from",
                    },
                    name_of(b),
                    name_of(pb),
                    name_of(pa),
                    name_of(pb)
                ),
                vec![e.id, pa, pb],
            );
        }
    }
    let package_edges: Vec<(Id, Id)> = package_uses.iter().copied().collect();
    for cycle in cycles(&package_edges) {
        let names: Vec<String> = cycle.iter().map(|id| name_of(*id)).collect();
        issue("L2", format!("Packages depend on each other in a cycle: {}.", names.join(" \u{2192} ")), cycle);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Member, ShapeKind};
    use egui::{pos2, vec2, Rect};

    fn rules(d: &Diagram) -> Vec<&'static str> {
        validate(d).into_iter().map(|i| i.rule).collect()
    }

    fn ptr(kind: PointerKind) -> LinkKind {
        LinkKind::Pointer(kind)
    }

    #[test]
    fn clean_diagram_has_no_issues() {
        let mut d = Diagram::default();
        let a = d.add_node(ShapeKind::Class, pos2(0.0, 0.0));
        let b = d.add_node(ShapeKind::Class, pos2(400.0, 0.0));
        d.node_mut(a).unwrap().text = "Owner".into();
        d.node_mut(b).unwrap().text = "Part".into();
        let e = d.add_link(End::Node(a), End::Node(b), ptr(PointerKind::Unique), false);
        d.edge_mut(e).unwrap().label = "part".into();
        assert_eq!(rules(&d), Vec::<&str>::new());
    }

    #[test]
    fn names_labels_and_members() {
        let mut d = Diagram::default();
        let a = d.add_node(ShapeKind::Class, pos2(0.0, 0.0));
        let b = d.add_node(ShapeKind::Class, pos2(400.0, 0.0));
        d.node_mut(a).unwrap().text = "Same".into();
        d.node_mut(b).unwrap().text = "Same".into();
        let e1 = d.add_link(End::Node(a), End::Node(b), ptr(PointerKind::Raw), false);
        d.add_link(End::Node(a), End::point(pos2(900.0, 0.0)), ptr(PointerKind::Raw), false);
        d.edge_mut(e1).unwrap().label = "x".into();
        let mut f = Member::new(MemberKind::Field, ShapeKind::Class, &[]);
        f.name = "x".into();
        d.node_mut(a).unwrap().members.push(f);
        let r = rules(&d);
        for want in ["O2", "P2", "P3", "M4", "M7"] {
            assert!(r.contains(&want), "missing {want} in {r:?}");
        }
    }

    #[test]
    fn shared_cycle_weak_without_shared_and_inheritance_cycle() {
        let mut d = Diagram::default();
        let ids: Vec<Id> = (0..3)
            .map(|i| {
                let id = d.add_node(ShapeKind::Class, pos2(i as f32 * 300.0, 0.0));
                d.node_mut(id).unwrap().text = format!("C{i}");
                id
            })
            .collect();
        for (a, b, k) in [(0, 1, PointerKind::Shared), (1, 0, PointerKind::Shared), (2, 2, PointerKind::Weak)] {
            let e = d.add_link(End::Node(ids[a]), End::Node(ids[b]), ptr(k), false);
            d.edge_mut(e).unwrap().label = format!("p{a}{b}");
        }
        d.add_link(End::Node(ids[0]), End::Node(ids[2]), LinkKind::Relation(RelationKind::Inherits), false);
        d.add_link(End::Node(ids[2]), End::Node(ids[0]), LinkKind::Relation(RelationKind::Inherits), false);
        let r = rules(&d);
        for want in ["P9", "P8", "R5"] {
            assert!(r.contains(&want), "missing {want} in {r:?}");
        }
    }

    #[test]
    fn package_dependencies_must_be_declared_and_acyclic() {
        let mut d = Diagram::default();
        let web = d.add_node(ShapeKind::Package, pos2(0.0, 0.0));
        let store = d.add_node(ShapeKind::Package, pos2(1000.0, 0.0));
        d.node_mut(web).unwrap().text = "web".into();
        d.node_mut(store).unwrap().text = "store".into();
        let a = d.add_node(ShapeKind::Class, pos2(0.0, 20.0));
        let b = d.add_node(ShapeKind::Class, pos2(1000.0, 20.0));
        d.node_mut(a).unwrap().text = "Handler".into();
        d.node_mut(b).unwrap().text = "Store".into();
        assert_eq!(d.package_of(a), Some(web));
        let p = d.add_link(End::Node(a), End::Node(b), ptr(PointerKind::Shared), false);
        d.edge_mut(p).unwrap().label = "store".into();
        assert!(rules(&d).contains(&"L1"));

        d.add_link(End::Node(web), End::Node(store), LinkKind::Relation(RelationKind::Uses), false);
        assert!(!rules(&d).contains(&"L1"), "declared now");

        d.add_link(End::Node(store), End::Node(web), LinkKind::Relation(RelationKind::Uses), false);
        assert!(rules(&d).contains(&"L2"));
        // A package frame is bigger than its contents.
        let frame = d.node(web).unwrap().rect();
        assert!(frame.contains_rect(Rect::from_min_size(pos2(-150.0, -100.0), vec2(10.0, 10.0))));
    }

    #[test]
    fn enums_and_collections() {
        let mut d = Diagram::default();
        let e = d.add_node(ShapeKind::Enum, pos2(0.0, 0.0));
        let v = d.add_node(ShapeKind::Vector, pos2(400.0, 0.0));
        d.node_mut(v).unwrap().text.clear();
        let r = rules(&d);
        assert!(r.contains(&"E1") && r.contains(&"C3"), "{r:?}");
        d.node_mut(e).unwrap().members.push(Member::new(MemberKind::Variant, ShapeKind::Enum, &[]));
        assert!(!rules(&d).contains(&"E1"));
    }
}
