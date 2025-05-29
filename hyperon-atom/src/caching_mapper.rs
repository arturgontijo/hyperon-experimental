use std::collections::HashMap;

#[derive(Clone)]
pub struct CachingMapper<K: Clone + std::hash::Hash + Eq + ?Sized, V: Clone, F: Fn(K) -> V> {
    mapper: F,
    mapping: HashMap<K, V>,
}

impl<K: Clone + std::hash::Hash + Eq + ?Sized, V: Clone, F: Fn(K) -> V> CachingMapper<K, V, F> {
    pub fn new(mapper: F) -> Self {
        Self{ mapper, mapping: HashMap::new() }
    }

    pub fn replace(&mut self, key: K) -> V {
        match self.mapping.get(&key) {
            Some(mapped) => mapped.clone(),
            None => {
                let new_val = (self.mapper)(key.clone());
                self.mapping.insert(key, new_val.clone());
                new_val
            }
        }
    }

    pub fn mapping(&self) -> &HashMap<K, V> {
        &self.mapping
    }

    pub fn mapping_mut(&mut self) -> &mut HashMap<K, V> {
        &mut self.mapping
    }

    pub fn as_fn_mut<'a>(&'a mut self) -> impl 'a + FnMut(K) -> V {
        move |k| { self.replace(k) }
    }
}
