use ecs::Entity;

fn main() {

    let entity = Entity::new(1, 0);
    println!("Entity: {}", entity);
    println!("Packed bits: 0x{:016X}", entity.to_bits());
}