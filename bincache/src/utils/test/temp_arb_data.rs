pub fn create_arb_data(range: usize) -> Vec<u8> {
    let mut vec = Vec::with_capacity(range);
    for i in 0..range {
        vec.push((i % 255) as u8);
    }
    vec
}
