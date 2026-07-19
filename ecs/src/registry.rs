use crate::{Entity, component::{ComponentId, ComponentInfo}, id::{AtomicAllocIdPool, AtomicIdAllocator}, sparse::SparseSet, store::{Store, TableType}};

pub struct Registry {
    entity_alloc: AtomicAllocIdPool<Entity>,
    compoent_info: SparseSet<ComponentId, ComponentInfo>,
    store: Store,
}

impl Registry {

    pub fn register_component(&mut self, component_info: &ComponentInfo) {
        self.compoent_info.insert(component_info.id, component_info.clone());
    }

    pub fn alloc_entity(&self) -> Entity {
        self.entity_alloc.allocate_atomic()
    }

    pub fn spawn(&mut self, entity: Entity, table_type: &TableType, components: &[Option<&[u8]>]) {

    }

    pub fn insert(&mut self, entity: Entity, component_id: ComponentId, component: Option<&[u8]>) {

    }

    pub fn remove(&mut self, entity: Entity, component_id: ComponentId) {

    }

    pub fn despawn(&mut self, entity: Entity) {

    }


}
