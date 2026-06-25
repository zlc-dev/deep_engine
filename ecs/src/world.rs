use crate::{Entity, id::{AtomicAllocIdPool, AtomicIdAllocator, DefaultIdPool, IdAllocator}, store::Store};

pub struct World {
    entity_alloc: AtomicAllocIdPool<Entity>,
    store: Box<Store>,
}

impl World {
    pub fn new() -> Self {
        Self {
            entity_alloc: AtomicAllocIdPool::new(),
            store: Box::new(Store::new()),
        }
    }

    pub fn alloc_entity(&self) -> Entity {
        self.entity_alloc.allocate_atomic()
    }

}

