//! Evidence-scoped calculations for Splatoon 3 Salmon Run 11.3.0.
//!
//! Inputs such as candidate eligibility and queue branch are supplied by the
//! caller. Outputs are nominal predictions, not observed in-game results.

pub mod applied_preview;
pub mod applied_wave;
pub mod artillery_stage;
pub mod carrier;
pub mod difficulty;
pub mod direction;
pub mod enemy_object;
pub mod event_zero_counter;
pub mod event_zero_location;
pub mod event_zero_runner;
pub mod first_rare;
pub mod follower_budget;
pub mod levels;
pub mod ordinary_stream;
pub mod placement_allocation;
pub mod preview;
pub mod private_weapons;
pub mod quota;
pub mod rare_request_position;
pub mod records;
pub mod relay;
pub mod rng;
pub mod rules;
pub mod rush_direction;
pub mod rush_location;
pub mod scenario_seed;
pub mod scenario_weapon;
pub mod schedule;
pub mod selected_wave_rare;
pub mod spawn_attempts;
pub mod spawn_position;
pub mod specials;
pub mod species;
pub mod spot;
pub mod stage_data;
pub mod stage_wave;
pub mod tamaire;
pub mod tower_stage;
pub mod wave;
pub mod wave_location;
pub mod weapon_catalog;
pub mod weapon_preprocess;
pub mod weapon_preview;
pub mod weapons;
pub mod zako_spot;
pub mod zako_timer;

pub use rng::{SeedWords, derive};
