use super::*;

#[test]
fn generated_ids_are_unique_inside_one_exchange() {
    let ids: BTreeSet<_> = (0..64).map(|_| generated_id("rpc")).collect();
    assert_eq!(ids.len(), 64);
}
