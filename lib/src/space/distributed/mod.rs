// DAS
use tokio::sync::Mutex;
use std::{
    fmt::{Debug, Display},
    sync::Arc
};

use das::DASNode;
use crate::{
    common::FlexRef,
    matcher::BindingsSet,
    metta::runner::stdlib::das::query_with_das,
    Atom
};

use super::{
    grounding::index::AtomIndex,
    Space,
    SpaceCommon,
    SpaceEvent,
    SpaceMut,
    SpaceVisitor
};

#[derive(Clone)]
pub struct DistributedAtomSpace {
    index: AtomIndex,
    common: SpaceCommon,
    node: Arc<Mutex<DASNode>>,
    name: Option<String>,
}

impl DistributedAtomSpace {
    pub fn new(node: Arc<Mutex<DASNode>>, name: Option<String>) -> Self {
        Self {
            index: AtomIndex::new(),
            common: SpaceCommon::default(),
            node,
            name,
        }
    }

    pub fn query(&self, query: &Atom) -> BindingsSet {
        query_with_das(self.name.clone(), &self.node, query)
    }

    pub fn add(&mut self, atom: Atom) {
        self.index.insert(atom.clone());
        self.common.notify_all_observers(&SpaceEvent::Add(atom));
    }

    pub fn remove(&mut self, atom: &Atom) -> bool {
        let is_removed = self.index.remove(atom);
        if is_removed {
            self.common.notify_all_observers(&SpaceEvent::Remove(atom.clone()));
        }
        is_removed
    }

    pub fn replace(&mut self, from: &Atom, to: Atom) -> bool {
        let is_replaced = self.index.remove(from);
        if is_replaced {
            self.index.insert(to.clone());
            self.common.notify_all_observers(&SpaceEvent::Replace(from.clone(), to));
        }
        is_replaced
    }
}

impl Space for DistributedAtomSpace {
    fn common(&self) -> FlexRef<SpaceCommon> {
        FlexRef::from_simple(&self.common)
    }
    fn query(&self, query: &Atom) -> BindingsSet {
        self.query(query)
    }
    fn atom_count(&self) -> Option<usize> {
        Some(self.index.iter().count())
    }
    fn visit(&self, v: &mut dyn SpaceVisitor) -> Result<(), ()> {
       Ok(self.index.iter().for_each(|atom| v.accept(atom)))
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

impl SpaceMut for DistributedAtomSpace {
    fn add(&mut self, atom: Atom) {
        self.add(atom)
    }
    fn remove(&mut self, atom: &Atom) -> bool {
        self.remove(atom)
    }
    fn replace(&mut self, from: &Atom, to: Atom) -> bool {
        self.replace(from, to)
    }
    fn as_space<'a>(&self) -> &(dyn Space + 'a) {
        self
    }
}

impl PartialEq for DistributedAtomSpace {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl Debug for DistributedAtomSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.name {
            Some(name) => write!(f, "DistributedAtomSpace-{name} ({self:p})"),
            None => write!(f, "DistributedAtomSpace-{self:p}")
        }
    }
}

impl Display for DistributedAtomSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.name {
            Some(name) => write!(f, "DistributedAtomSpace-{name}"),
            None => write!(f, "DistributedAtomSpace-{self:p}")
        }
    }
}

// impl Grounded for DistributedAtomSpace {
//     fn type_(&self) -> Atom {
//         rust_type_atom::<DistributedAtomSpace>()
//     }

//     fn as_match(&self) -> Option<&dyn CustomMatch> {
//         Some(self)
//     }
// }

// impl CustomMatch for DistributedAtomSpace {
//     fn match_(&self, other: &Atom) -> matcher::MatchResultIter {
//         Box::new(self.query(other).into_iter())
//     }
// }
