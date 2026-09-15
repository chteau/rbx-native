//! Builds the class/referent/parent layout the rest of the serializer writes from.
//!
//! Traversal order only has to be *consistent* between the INST and PROP arrays of a
//! class (the reader matches them up positionally); it does not need to match how the
//! file was originally read. A single deterministic preorder walk from the roots gives
//! every chunk a stable, reproducible order.

use std::collections::HashMap;

use rbx_dom::{Ref, WeakDom};

use super::service;

/// One class's INST chunk worth of information: its assigned id and every referent of
/// that class, in the order PROP arrays for this class must use.
pub(crate) struct ClassPlan {
    pub(crate) class_id: i32,
    pub(crate) class_name: String,
    pub(crate) is_service: bool,
    pub(crate) referents: Vec<Ref>,
}

pub(crate) struct Plan {
    // In first-seen order, which becomes each class's `class_id`.
    pub(crate) classes: Vec<ClassPlan>,
    // Every instance in the file, in the order PRNT's arrays are written.
    pub(crate) order: Vec<Ref>,
    // Absent entry means "root": PRNT writes -1 for it.
    pub(crate) parents: HashMap<Ref, Ref>,
}

pub(crate) fn build(dom: &WeakDom) -> Plan {
    let order = visit_order(dom);

    let mut parents = HashMap::new();
    for &referent in &order {
        if let Some(instance) = dom.get(referent) {
            for &child in instance.children() {
                parents.insert(child, referent);
            }
        }
    }

    let mut classes: Vec<ClassPlan> = Vec::new();
    let mut class_index: HashMap<&str, usize> = HashMap::new();
    for &referent in &order {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        let class_name = instance.class();

        let index = *class_index.entry(class_name).or_insert_with(|| {
            classes.push(ClassPlan {
                class_id: classes.len() as i32,
                class_name: class_name.to_owned(),
                is_service: service::is_service(class_name),
                referents: Vec::new(),
            });
            classes.len() - 1
        });
        classes[index].referents.push(referent);
    }

    Plan {
        classes,
        order,
        parents,
    }
}

// Iterative preorder DFS (root, its children, their children, ...): a recursive walk
// would risk a stack overflow on a pathologically deep tree.
fn visit_order(dom: &WeakDom) -> Vec<Ref> {
    let mut order = Vec::new();
    let mut stack: Vec<Ref> = dom.root_refs().iter().rev().copied().collect();

    while let Some(referent) = stack.pop() {
        order.push(referent);
        if let Some(instance) = dom.get(referent) {
            stack.extend(instance.children().iter().rev().copied());
        }
    }

    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::Instance;

    #[test]
    fn groups_instances_by_class_and_assigns_ids_in_first_seen_order() {
        let mut dom = WeakDom::new();
        let a = dom.new_instance("Folder", "A", None);
        let b = dom.new_instance("Part", "B", Some(a));
        let c = dom.new_instance("Folder", "C", Some(a));

        let plan = build(&dom);

        assert_eq!(plan.classes.len(), 2);
        assert_eq!(plan.classes[0].class_name, "Folder");
        assert_eq!(plan.classes[0].referents, vec![a, c]);
        assert_eq!(plan.classes[1].class_name, "Part");
        assert_eq!(plan.classes[1].referents, vec![b]);
        assert_eq!(plan.parents.get(&b), Some(&a));
        assert_eq!(plan.parents.get(&a), None);
    }

    #[test]
    fn visit_order_is_a_preorder_walk() {
        let mut dom = WeakDom::new();
        let root = dom.new_instance("Folder", "Root", None);
        let child = dom.new_instance("Folder", "Child", Some(root));
        let grandchild = dom.new_instance("Folder", "Grandchild", Some(child));

        assert_eq!(visit_order(&dom), vec![root, child, grandchild]);
    }

    #[test]
    fn is_service_is_looked_up_per_class() {
        let mut dom = WeakDom::new();
        dom.insert(Instance::new(Ref::new(1), "Workspace", "Workspace"));
        dom.insert(Instance::new(Ref::new(2), "Part", "Part"));

        let plan = build(&dom);

        let workspace = plan
            .classes
            .iter()
            .find(|c| c.class_name == "Workspace")
            .unwrap();
        let part = plan
            .classes
            .iter()
            .find(|c| c.class_name == "Part")
            .unwrap();
        assert!(workspace.is_service);
        assert!(!part.is_service);
    }
}
