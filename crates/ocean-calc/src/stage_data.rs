//! Static 11.3.0 Banc spawn candidates, in the source file's `Actors` order.
//!
//! Under the traced normal actor-system configuration, successful Banc actor
//! creations register synchronously and preserve their relative source order.
//! Runtime filtering, other actor sources, and later list changes still prevent
//! assuming that every final candidate index matches this table. These are
//! actor bases, not final enemy XYZ.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnActor {
    pub actor_index: u16,
    pub hash: u64,
    pub translate: [f32; 3],
    /// Raw BYML value; `None` means the actor omits `Rotate`.
    pub rotate: Option<[f32; 3]>,
}

/// One raw `CoopSpawnGeyser` Banc actor that may feed Event 8's tide lists.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayeredSpawnActor {
    /// Parsed `Layer`: Low=0, Mid=1, High=2.
    pub layer: u8,
    pub actor: SpawnActor,
}

/// Raw Banc `Layer` for `CoopIkuraBank` and `CoopIkuraBankBase`.
/// No active-tide rule is implied by this value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BankLayer {
    Common,
    Low,
    Mid,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayeredBankActor {
    pub layer: BankLayer,
    pub actor: SpawnActor,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PillarAnchor {
    pub actor_index: u16,
    pub hash: u64,
    /// Raw Banc base transform, not the callback's runtime transform vectors.
    pub translate: [f32; 3],
    pub rotate: Option<[f32; 3]>,
    /// Linked `CoopSakePillarArrivalPoint` actors, in the anchor's link order.
    pub arrivals: &'static [SpawnActor],
    /// Link destinations not found among this Banc's arrival actors.
    pub dangling_destinations: &'static [u64],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PillarStaticGate {
    /// Every possible chosen anchor in at least one fallback direction has a
    /// linked arrival actor. Assumes normal runtime actor construction.
    Available,
    /// No static linked arrival actor exists for this tide.
    Unavailable,
    /// An overwritten duplicate anchor's runtime choice could affect this.
    DependsOnRuntimeOrder,
}

#[derive(Clone, Copy, Debug)]
pub struct StageSpawns {
    pub name: &'static str,
    pub pack_sha256: &'static str,
    pub banc_path: &'static str,
    /// Stage weights are present only when `IsOverwriteWater` is effectively
    /// true and the stage component supplies a `Water` list.
    pub water_override: Option<[u32; 3]>,
    /// Bit `1 << event_id` denotes a `BanEvent` from the stage component.
    pub ban_event_mask: u32,
    /// Index `3 * direction + tide`, with tide Low/Mid/High = 0/1/2.
    pub groups: [&'static [SpawnActor]; 9],
    /// Raw Artillery gun point actors by `3 * direction + tide` in Banc order.
    /// Runtime registration order is not proven to match this order.
    pub artillery_points: [&'static [SpawnActor]; 9],
    /// Raw Tower arrival actors by `3 * direction + tide` in Banc order.
    /// The runtime callback filters and orders registered actors separately.
    pub tower_arrivals: [&'static [SpawnActor]; 9],
    /// Raw `CoopSpawnGeyser` actors in Banc `Actors` order. Runtime actor
    /// registration and the Event 8 node transform remain unverified.
    pub tamaire_geysers: &'static [LayeredSpawnActor],
    /// Raw Banc `CoopIkuraBank` actors in source order. Runtime activation
    /// and the exact GUI marker anchor have not been established.
    pub ikura_banks: &'static [LayeredBankActor],
    /// Raw Banc `CoopIkuraBankBase` actors in source order, kept separately
    /// from the Bank actors because their Y coordinates can differ.
    pub ikura_bank_bases: &'static [LayeredBankActor],
    /// Raw Event 4 `CoopSpawnBoxLocator` actors by exclusive Low/Mid/High layer.
    /// Runtime registration and transform equivalence remain conditional.
    pub carrier_box_locators: [&'static [SpawnActor]; 3],
    /// Pillar anchors and their linked arrival actors in Banc order. Duplicate
    /// anchors remain separate because the game's runtime group pointer can
    /// be overwritten according to scene actor enumeration order.
    pub pillar_anchors: [&'static [PillarAnchor]; 9],
}

impl StageSpawns {
    /// Return a Banc-order list for a tide and direction in `0..=2`.
    pub fn group(&self, tide: u8, direction: u8) -> Option<&'static [SpawnActor]> {
        if tide > 2 || direction > 2 {
            return None;
        }
        Some(self.groups[usize::from(3 * direction + tide)])
    }

    /// Return the raw Artillery point actors for a tide and direction.
    pub fn artillery_group(&self, tide: u8, direction: u8) -> Option<&'static [SpawnActor]> {
        if tide > 2 || direction > 2 {
            return None;
        }
        Some(self.artillery_points[usize::from(3 * direction + tide)])
    }

    /// Return the raw Tower arrival actors for a tide and direction.
    pub fn tower_arrival_group(&self, tide: u8, direction: u8) -> Option<&'static [SpawnActor]> {
        if tide > 2 || direction > 2 {
            return None;
        }
        Some(self.tower_arrivals[usize::from(3 * direction + tide)])
    }

    /// Conditional Event 8 source membership in Banc order for tide 0–2.
    ///
    /// This is the builder's Layer admission (`layer >= tide`) if these
    /// actors register normally and in Banc order. The game falls back to
    /// tide 0 when its runtime tide is absent or invalid; this static accessor
    /// instead returns `None` for an invalid caller-supplied tide. Raw Banc
    /// transforms are not the live 56-byte nodes or final spawn positions.
    pub fn tamaire_geyser_sources(&self, tide: u8) -> Option<Vec<LayeredSpawnActor>> {
        if tide > 2 {
            return None;
        }
        Some(
            self.tamaire_geysers
                .iter()
                .copied()
                .filter(|source| source.layer >= tide)
                .collect(),
        )
    }

    /// Raw Event 4 box locators for an already validated tide in `0..=2`.
    ///
    /// The carrier's runtime tide lookup falls back to Low for a missing or
    /// invalid scene tide; this accessor instead rejects an invalid input.
    /// The actor base transform is not the final enemy position.
    pub fn carrier_box_sources(&self, tide: u8) -> Option<&'static [SpawnActor]> {
        self.carrier_box_locators.get(usize::from(tide)).copied()
    }

    /// Keys present in the static Banc snapshot, for `direction::choose_direction`.
    pub fn available_keys(&self) -> Vec<u8> {
        self.groups
            .iter()
            .enumerate()
            .filter(|(_, group)| !group.is_empty())
            .map(|(key, _)| key as u8)
            .collect()
    }

    /// Unique source anchor for ID11's first untouched requested group.
    ///
    /// This assumes the stage actors registered normally, WAVE init cleared
    /// used flags, and no earlier Pillar state update ran. `None` includes
    /// missing or duplicate groups and anchors without resolved arrivals.
    /// The returned raw Banc transform is not the callback's output vectors
    /// or the final spawned enemy coordinate.
    pub fn pillar_first_untouched_anchor(
        &self,
        tide: u8,
        direction: u8,
    ) -> Option<&'static PillarAnchor> {
        if tide > 2 || direction > 2 {
            return None;
        }
        let anchors = self.pillar_anchors[usize::from(3 * direction + tide)];
        let [anchor] = anchors else {
            return None;
        };
        (!anchor.arrivals.is_empty()).then_some(anchor)
    }

    /// Whether ID11's first untouched spot callback is guaranteed to handle
    /// the requested tide/direction group under normal actor registration.
    ///
    /// Unlike the candidate callback, the spot callback returns false at a
    /// missing requested group. Duplicate anchors are acceptable here only
    /// if every possible registered anchor has a linked arrival. `false`
    /// means the route remains unproven, not necessarily that it declines.
    pub fn pillar_initial_spot_guaranteed(&self, tide: u8, direction: u8) -> bool {
        if tide > 2 || direction > 2 {
            return false;
        }
        let anchors = self.pillar_anchors[usize::from(3 * direction + tide)];
        !anchors.is_empty() && anchors.iter().all(|anchor| !anchor.arrivals.is_empty())
    }

    /// Static part of ID11's WAVE-start candidate callback.
    ///
    /// WAVE initialization clears all Pillar used flags, and the candidate
    /// callback searches the requested direction then the other two. This
    /// differs from the spot callback's missing-group early return. This answer
    /// assumes all referenced scene actors were registered and the runtime
    /// Pillar groups were built from this Banc. Later used flags, duplicate
    /// anchor overwrite order, and runtime actor filtering are not inferred.
    pub fn pillar_initial_static_gate(&self, tide: u8, direction: u8) -> Option<PillarStaticGate> {
        if tide > 2 || direction > 2 {
            return None;
        }
        let mut possibly_available = false;
        for current_direction in
            std::iter::once(direction).chain((0..3).filter(|d| *d != direction))
        {
            let anchors = self.pillar_anchors[usize::from(3 * current_direction + tide)];
            if anchors.is_empty() {
                continue;
            }
            if anchors.iter().all(|anchor| !anchor.arrivals.is_empty()) {
                return Some(PillarStaticGate::Available);
            }
            if anchors.iter().any(|anchor| !anchor.arrivals.is_empty()) {
                possibly_available = true;
            }
        }
        Some(if possibly_available {
            PillarStaticGate::DependsOnRuntimeOrder
        } else {
            PillarStaticGate::Unavailable
        })
    }
}

include!("stage_data_generated.rs");

pub fn all_stages() -> &'static [StageSpawns] {
    GENERATED_STAGES
}

/// Look up an exact `Cop_*` resource name in the 11.3.0 static snapshot.
pub fn stage_spawns(name: &str) -> Option<&'static StageSpawns> {
    GENERATED_STAGES.iter().find(|stage| stage.name == name)
}
