use salmon_wave_lookahead::{SeedWords, derive, rng::XorShift128};

#[test]
fn scene_zero_seed_becomes_nonzero_shared_state() {
    assert_eq!(SeedWords::from_scene([0; 4]).as_array(), [1, 0, 0, 0]);
}

#[test]
fn schedule_domain_derives_the_disassembled_four_words() {
    let seed = SeedWords::from_scene([1, 2, 3, 4]);
    let state = derive(seed, 0xe6da_1c4b);
    assert_eq!(
        state.as_array(),
        [0x3402_0ae8, 0xc8fc_758a, 0x693f_b010, 0x97d0_8fb9]
    );
}

#[test]
fn xorshift_advances_all_four_words() {
    let mut state = XorShift128::from_words([0x3402_0ae8, 0xc8fc_758a, 0x693f_b010, 0x97d0_8fb9]);
    assert_eq!(state.next32(), 0xb3a1_82e1);
    assert_eq!(
        state.as_array(),
        [0xc8fc_758a, 0x693f_b010, 0x97d0_8fb9, 0xb3a1_82e1]
    );
}
