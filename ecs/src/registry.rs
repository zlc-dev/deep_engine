use crate::{Entity, id::AtomicAllocIdPool, store::Store};


pub struct Registry {
    entity_alloc: AtomicAllocIdPool<Entity>,
    store: Store
}
