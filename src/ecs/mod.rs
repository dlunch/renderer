use alloc::boxed::Box;
use core::{
    any::{Any, TypeId},
    ops::{Deref, DerefMut},
};

mod hierarchy;
mod sparse_vec;

use sparse_vec::SparseVec;

type ComponentType = TypeId;

pub struct World {
    components: SparseVec<SparseVec<Box<dyn Component>, Entity>, ComponentType>,
    entities: u32,
}

impl World {
    pub fn new() -> Self {
        Self {
            components: SparseVec::new(),
            entities: 0,
        }
    }

    pub fn spawn(&mut self) -> Entity {
        let id = self.entities;

        self.entities += 1;

        Entity { id }
    }

    pub fn add_component<T: 'static + Component>(&mut self, entity: Entity, component: T) {
        let component_type = TypeId::of::<T>();

        let vec = if let Some(x) = self.components.get_mut(component_type) {
            x
        } else {
            let vec = SparseVec::new();
            self.components.insert(component_type, vec);

            self.components.get_mut(component_type).unwrap()
        };

        vec.insert(entity, Box::new(component));
    }

    pub fn component<T: 'static + Component>(&self, entity: Entity) -> Option<&T> {
        let component_type = TypeId::of::<T>();

        let item = self.components.get(component_type)?.get(entity)?;
        Some(item.deref().as_any().downcast_ref::<T>().unwrap())
    }

    pub fn component_mut<T: 'static + Component>(&mut self, entity: Entity) -> Option<&mut T> {
        let component_type = TypeId::of::<T>();

        let item = self.components.get_mut(component_type)?.get_mut(entity)?;
        Some(item.deref_mut().as_any_mut().downcast_mut::<T>().unwrap())
    }

    pub fn components<T: 'static + Component>(&self) -> impl Iterator<Item = (Entity, &T)> {
        let component_type = TypeId::of::<T>();

        self.components
            .get(component_type)
            .unwrap()
            .iter()
            .map(|(entity, component)| (*entity, component.deref().as_any().downcast_ref::<T>().unwrap()))
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct Entity {
    id: u32,
}

pub trait AsAny {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: 'static> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub trait Component: AsAny {}

#[cfg(test)]
mod test {
    use super::{Component, World};

    #[test]
    fn test_entity() {
        let mut world = World::new();

        world.spawn();
    }

    #[test]
    fn test_component() {
        struct TestComponent {
            test: u32,
        }

        impl Component for TestComponent {}

        let mut world = World::new();
        let entity = world.spawn();

        world.add_component(entity, TestComponent { test: 1 });
        assert_eq!(world.component::<TestComponent>(entity).unwrap().test, 1);
    }

    #[test]
    fn test_components() {
        struct TestComponent {
            test: u32,
        }

        impl Component for TestComponent {}

        let mut world = World::new();

        let entity = world.spawn();
        world.add_component(entity, TestComponent { test: 1 });

        let entity = world.spawn();
        world.add_component(entity, TestComponent { test: 2 });

        let mut it = world.components::<TestComponent>();
        assert_eq!(it.next().unwrap().1.test, 1);
        assert_eq!(it.next().unwrap().1.test, 2);
        assert!(it.next().is_none());
    }
}