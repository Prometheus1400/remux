use rand::Rng;

pub fn generate_id() -> u32 {
    let mut rng = rand::rng();
    rng.random()
}
