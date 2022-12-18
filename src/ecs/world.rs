use alloc::{boxed::Box, vec::Vec};
use core::{
    any::{Any, TypeId},
    cell::{Ref, RefCell, RefMut},
    future::Future,
    marker::PhantomData,
};

use futures::{future::BoxFuture, poll, task::Poll, FutureExt};
use hashbrown::{hash_map::Entry, HashMap};

use super::{builder::EntityBuilder, bundle::ComponentBundle, query::Query, sparse_raw_vec::SparseRawVec, Component, Entity};

type ComponentType = TypeId;
type ResourceType = TypeId;
type EventType = TypeId;

pub struct World {
    components: HashMap<ComponentType, SparseRawVec<Entity>>,
    resources: HashMap<ResourceType, RefCell<Box<dyn Any>>>,
    entities: u32,
    #[allow(clippy::type_complexity)]
    pending: Vec<(BoxFuture<'static, Box<dyn Any>>, Box<dyn SystemCallback>)>,
    event_handlers: HashMap<EventType, Vec<Box<dyn SystemCallback>>>,
}

impl World {
    pub fn new() -> Self {
        Self {
            components: HashMap::new(),
            resources: HashMap::new(),
            entities: 0,
            pending: Vec::new(),
            event_handlers: HashMap::new(),
        }
    }

    pub fn spawn(&mut self) -> EntityBuilder<'_> {
        let id = self.entities;

        self.entities += 1;

        EntityBuilder::new(self, Entity { id })
    }

    pub fn destroy(&mut self, entity: Entity) {
        for (_, storage) in self.components.iter_mut() {
            storage.remove(entity);
        }
    }

    pub fn spawn_bundle<T: 'static + ComponentBundle>(&mut self, bundle: T) -> Entity {
        let entity = self.spawn().entity();

        self.add_bundle(entity, bundle);

        entity
    }

    pub fn add_bundle<T: 'static + ComponentBundle>(&mut self, entity: Entity, bundle: T) {
        bundle.add_components(self, entity)
    }

    pub fn add_component<T: 'static + Component>(&mut self, entity: Entity, component: T) {
        let component_type = Self::get_component_type::<T>();

        let vec = if let Some(x) = self.components.get_mut(&component_type) {
            x
        } else {
            let vec = SparseRawVec::new::<T>();
            self.components.insert(component_type, vec);

            self.components.get_mut(&component_type).unwrap()
        };

        vec.insert(entity, component);
    }

    pub fn component<T: 'static + Component>(&self, entity: Entity) -> Option<&T> {
        let component_type = Self::get_component_type::<T>();

        self.components.get(&component_type)?.get::<T>(entity)
    }

    pub fn component_mut<T: 'static + Component>(&mut self, entity: Entity) -> Option<&mut T> {
        let component_type = Self::get_component_type::<T>();

        self.components.get_mut(&component_type)?.get_mut::<T>(entity)
    }

    pub fn components<T: 'static + Component>(&self) -> impl Iterator<Item = (Entity, &T)> {
        let component_type = Self::get_component_type::<T>();

        self.components.get(&component_type).unwrap().iter()
    }

    pub fn components_mut<T: 'static + Component>(&mut self) -> impl Iterator<Item = (Entity, &mut T)> {
        let component_type = Self::get_component_type::<T>();

        self.components.get_mut(&component_type).unwrap().iter_mut()
    }

    pub fn query<T: 'static + Query>(&self) -> impl Iterator<Item = Entity> + '_ {
        (0..self.entities).map(|x| Entity { id: x }).filter(|&x| T::matches(self, x))
    }

    pub fn has_component<T: 'static + Component>(&self, entity: Entity) -> bool {
        let component_type = Self::get_component_type::<T>();

        if let Some(components) = self.components.get(&component_type) {
            components.contains(entity)
        } else {
            false
        }
    }

    pub fn add_resource<T: 'static>(&mut self, resource: T) {
        let resource_type = Self::get_resource_type::<T>();

        self.resources.insert(resource_type, RefCell::new(Box::new(resource)));
    }

    pub fn resource<T: 'static>(&self) -> Option<Ref<'_, T>> {
        let resource_type = Self::get_resource_type::<T>();

        let storage = self.resources.get(&resource_type)?.borrow();

        Some(Ref::map(storage, |x| x.downcast_ref::<T>().unwrap()))
    }

    pub fn resource_mut<T: 'static>(&self) -> Option<RefMut<'_, T>> {
        let resource_type = Self::get_resource_type::<T>();

        let storage = self.resources.get(&resource_type)?.borrow_mut();

        Some(RefMut::map(storage, |x| x.downcast_mut::<T>().unwrap()))
    }

    pub fn take_resource<T: 'static>(&mut self) -> Option<T> {
        let resource_type = Self::get_resource_type::<T>();

        Some(*self.resources.remove(&resource_type)?.into_inner().downcast::<T>().unwrap())
    }

    pub fn async_job<Func, Fut, C, Ret>(&mut self, func: Func, callback: C)
    where
        Func: FnOnce() -> Fut,
        for<'a> Fut: Future<Output = Ret> + Sync + Send + 'a,
        C: Fn(&mut World, &Ret) + 'static,
        Ret: 'static,
    {
        let fut = func().map(|x| Box::new(x) as Box<dyn Any>).fuse().boxed();

        self.pending.push((fut, Box::new(SystemCallbackWrapper::new(callback))));
    }

    pub(crate) async fn update(&mut self) {
        let mut pending = Vec::with_capacity(self.pending.len());
        core::mem::swap(&mut self.pending, &mut pending);

        for (mut future, callback) in pending {
            if let Poll::Ready(x) = poll!(&mut future) {
                callback.call(self, &*x);
            } else {
                self.pending.push((future, callback));
            }
        }
    }

    pub fn add_event_handler<EventT, C>(&mut self, callback: C)
    where
        C: Fn(&mut World, &EventT) + 'static,
        EventT: 'static,
    {
        let event_type = Self::get_event_type::<EventT>();
        let value = Box::new(SystemCallbackWrapper::new(callback));

        let entry = self.event_handlers.entry(event_type);
        if let Entry::Occupied(mut entry) = entry {
            entry.get_mut().push(value);
        } else {
            entry.insert(vec![value]);
        }
    }

    pub(crate) fn on_event<EventT>(&mut self, event: EventT)
    where
        EventT: 'static,
    {
        let event_type = Self::get_event_type::<EventT>();

        let mut event_handlers = HashMap::new();
        core::mem::swap(&mut event_handlers, &mut self.event_handlers); // TODO remove

        if let Some(callbacks) = event_handlers.get(&event_type) {
            for callback in callbacks {
                callback.call(self, &event);
            }
        }

        core::mem::swap(&mut event_handlers, &mut self.event_handlers); // TODO remove
    }

    fn get_component_type<ComponentT>() -> ComponentType
    where
        ComponentT: Component + 'static,
    {
        TypeId::of::<ComponentT>()
    }

    fn get_resource_type<ResourceT>() -> ResourceType
    where
        ResourceT: 'static,
    {
        TypeId::of::<ResourceT>()
    }

    fn get_event_type<EventT>() -> EventType
    where
        EventT: 'static,
    {
        TypeId::of::<EventT>()
    }
}

pub struct SystemCallbackWrapper<F, T>(F, PhantomData<T>);

pub trait SystemCallback {
    fn call(&self, world: &mut World, args: &(dyn Any + 'static));
}

impl<F, T> SystemCallbackWrapper<F, T>
where
    SystemCallbackWrapper<F, T>: SystemCallback,
{
    pub fn new(f: F) -> Self {
        Self(f, PhantomData)
    }
}

impl<T, Ret> SystemCallback for SystemCallbackWrapper<T, Ret>
where
    T: Fn(&mut World, &Ret),
    Ret: 'static,
{
    fn call(&self, world: &mut World, args: &(dyn Any + 'static)) {
        let args = args.downcast_ref::<Ret>().unwrap();

        (self.0)(world, args);
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    use alloc::{vec, vec::Vec};

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
        let entity = world.spawn().with(TestComponent { test: 1 }).entity();

        assert_eq!(world.component::<TestComponent>(entity).unwrap().test, 1);
    }

    #[test]
    fn test_component_empty() {
        struct TestComponent {}

        impl Component for TestComponent {}

        let mut world = World::new();
        let entity = world.spawn().with(TestComponent {}).entity();

        assert!(world.has_component::<TestComponent>(entity));
        assert!(world.component::<TestComponent>(entity).is_some());
    }

    #[test]
    fn test_components() {
        struct TestComponent {
            test: u32,
        }

        impl Component for TestComponent {}

        let mut world = World::new();

        world.spawn().with(TestComponent { test: 1 }).entity();

        world.spawn().with(TestComponent { test: 2 }).entity();

        let mut it = world.components::<TestComponent>();
        assert_eq!(it.next().unwrap().1.test, 1);
        assert_eq!(it.next().unwrap().1.test, 2);
        assert!(it.next().is_none());
    }

    #[test]
    fn test_components_mut() {
        struct TestComponent {
            test: u32,
        }

        impl Component for TestComponent {}

        let mut world = World::new();

        world.spawn().with(TestComponent { test: 1 }).entity();

        world.spawn().with(TestComponent { test: 2 }).entity();

        {
            let mut it = world.components_mut::<TestComponent>();
            it.next().unwrap().1.test = 123;
        }

        let mut it = world.components::<TestComponent>();

        assert_eq!(it.next().unwrap().1.test, 123);
    }

    #[test]
    fn test_resource() {
        struct TestResource1 {
            a: u32,
        }
        struct TestResource2 {
            b: Vec<u32>,
        }
        let mut world = World::new();

        world.add_resource(TestResource1 { a: 123 });
        world.add_resource(TestResource2 { b: vec![1234] });

        assert_eq!(world.resource::<TestResource1>().unwrap().a, 123);
        assert_eq!(world.resource::<TestResource2>().unwrap().b, [1234]);
    }

    #[test]
    fn test_resource_overwrite() {
        struct TestResource {
            a: u32,
        }
        let mut world = World::new();

        world.add_resource(TestResource { a: 123 });
        assert_eq!(world.resource::<TestResource>().unwrap().a, 123);

        world.add_resource(TestResource { a: 1234 });
        assert_eq!(world.resource::<TestResource>().unwrap().a, 1234);
    }

    #[test]
    fn test_bundle() {
        struct TestComponent1 {
            a: u32,
        }
        impl Component for TestComponent1 {}
        struct TestComponent2 {
            a: u32,
        }
        impl Component for TestComponent2 {}

        let mut world = World::new();

        let bundle = (TestComponent1 { a: 1 }, TestComponent2 { a: 2 });
        let entity = world.spawn_bundle(bundle);

        assert_eq!(world.component::<TestComponent1>(entity).unwrap().a, 1);
        assert_eq!(world.component::<TestComponent2>(entity).unwrap().a, 2);
    }

    #[test]
    fn test_has_component() {
        struct TestComponent {}

        impl Component for TestComponent {}

        let mut world = World::new();

        let entity1 = world.spawn().with(TestComponent {}).entity();
        let entity2 = world.spawn().entity();

        assert!(world.has_component::<TestComponent>(entity1));
        assert!(!world.has_component::<TestComponent>(entity2));
    }

    #[test]
    fn test_quer1y() {
        struct TestComponent {}

        impl Component for TestComponent {}

        let mut world = World::new();

        let entity1 = world.spawn().with(TestComponent {}).entity();
        world.spawn().entity();

        let mut query = world.query::<(TestComponent,)>();
        assert!(query.next().unwrap() == entity1);
        assert!(query.next().is_none());
    }

    #[test]
    fn test_query2() {
        struct TestComponent1 {}
        impl Component for TestComponent1 {}
        struct TestComponent2 {}
        impl Component for TestComponent2 {}

        let mut world = World::new();

        let entity1 = world.spawn().with(TestComponent1 {}).with(TestComponent2 {}).entity();
        world.spawn().with(TestComponent1 {}).entity();

        let mut query = world.query::<(TestComponent1, TestComponent2)>();
        assert!(query.next().unwrap() == entity1);
        assert!(query.next().is_none());
    }

    #[test]
    fn test_destroy() {
        struct TestComponent {}

        impl Component for TestComponent {}

        let mut world = World::new();
        let entity = world.spawn().with(TestComponent {}).entity();

        world.destroy(entity);

        assert!(world.component::<TestComponent>(entity).is_none());
    }

    #[tokio::test]
    async fn test_async() {
        struct TestComponent {
            v: u32,
        }

        impl Component for TestComponent {}

        let mut world = World::new();

        world.async_job(
            || async { 1 },
            |world, &v| {
                world.spawn().with(TestComponent { v });
            },
        );

        world.update().await;

        assert_eq!(world.components::<TestComponent>().next().unwrap().1.v, 1);
    }
}
