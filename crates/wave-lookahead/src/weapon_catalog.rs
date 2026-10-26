//! Static RSDB eligibility for JobType 0/1 weapon-pool builders.
//!
//! The source candidate APIs retain the owned RSDB root-array order. The
//! game's registry field index 8 sorts the owned 331 distinct source IDs in
//! signed ascending order. The effective resolvers apply that order plus
//! caller-supplied runtime lookup outcomes, a signed Season cap read at each
//! builder call, and each pool's signed capacity. Their low-16-bit IDs feed the draw functions in
//! [`crate::weapons`]; they consume no weapon RNG.

#[path = "weapon_pool_data_generated.rs"]
mod generated;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceWeaponCandidate {
    /// Position in the owned RSDB root array, not a runtime pool index.
    pub source_index: u16,
    pub source_row_id: &'static str,
    pub source_id: u16,
    /// The Coop row referenced by a Versus source; absent for third-pool rows.
    pub target_row_id: Option<&'static str>,
    /// Coop ID eligible to be appended if runtime lookup and capacity permit.
    pub candidate_id: u16,
    /// Signed source-row Season used by the ELF cap comparison.
    pub season: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourcePoolKind {
    Normal,
    ThirdRare,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RsdbOrderedCandidates {
    pub kind: SourcePoolKind,
    /// Eligible source rows in RSDB root order, not the game's sorted pool order.
    pub entries: Vec<SourceWeaponCandidate>,
    pub source_sha256: &'static str,
    /// The game appends this ID only if the effective pool is empty *and* its
    /// runtime capacity is positive. Runtime lookups can also omit rows.
    pub conditional_empty_fallback_id: u16,
}

/// Provenance aligned by index with an effective pool's draw-visible IDs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectivePoolSource {
    Source(SourceWeaponCandidate),
    Fallback,
}

/// Conditional pool in the game's field-index-8 iteration order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveWeaponPool {
    pub kind: SourcePoolKind,
    /// Full 32-bit IDs stored in pool nodes and used by source validation.
    pub stored_ids: Vec<i32>,
    /// IDs observed by the downstream `ldrh` draw, one for each appended node.
    pub draw_ids: Vec<u16>,
    /// The source row or fallback for each ID, at the same index.
    pub sources: Vec<EffectivePoolSource>,
    pub source_sha256: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PoolResolutionError<E> {
    /// The normal Coop target lookup outcome is unknown at this source row.
    UnknownLookup {
        source: SourceWeaponCandidate,
        error: E,
    },
}

/// Resolve the normal helper pool using one lookup outcome per eligible row.
///
/// The callback runs in signed source-ID order after the Season filter,
/// including when capacity is zero or already full. `Ok(None)` is a known
/// null lookup; an error stops resolution without inventing a pool. The
/// callback returns the looked-up Coop row's full 32-bit node ID.
pub fn resolve_normal_pool<E>(
    season_cap: i32,
    signed_capacity: i32,
    mut lookup: impl FnMut(SourceWeaponCandidate) -> Result<Option<i32>, E>,
) -> Result<EffectiveWeaponPool, PoolResolutionError<E>> {
    let mut rows = normal_source_candidates(season_cap).entries;
    rows.sort_by_key(|row| i32::from(row.source_id));
    let mut pool = empty_effective_pool(SourcePoolKind::Normal);
    for row in rows {
        let resolved = lookup(row)
            .map_err(|error| PoolResolutionError::UnknownLookup { source: row, error })?;
        if let Some(id) = resolved
            && (pool.draw_ids.len() as i32) < signed_capacity
        {
            pool.stored_ids.push(id);
            pool.draw_ids.push(id as u16);
            pool.sources.push(EffectivePoolSource::Source(row));
        }
    }
    append_empty_fallback(&mut pool, signed_capacity, 20040);
    Ok(pool)
}

/// Resolve the rare third pool from static source rows at this call's cap.
///
/// This branch does not perform a Coop target lookup. It appends the source
/// row's low-16-bit ID while capacity remains, preserving signed-ID order.
pub fn resolve_third_pool(season_cap: i32, signed_capacity: i32) -> EffectiveWeaponPool {
    let mut rows = third_source_candidates(season_cap).entries;
    rows.sort_by_key(|row| i32::from(row.source_id));
    let mut pool = empty_effective_pool(SourcePoolKind::ThirdRare);
    for row in rows {
        if (pool.draw_ids.len() as i32) < signed_capacity {
            pool.stored_ids.push(i32::from(row.source_id));
            pool.draw_ids.push(row.source_id);
            pool.sources.push(EffectivePoolSource::Source(row));
        }
    }
    append_empty_fallback(&mut pool, signed_capacity, 20900);
    pool
}

fn empty_effective_pool(kind: SourcePoolKind) -> EffectiveWeaponPool {
    EffectiveWeaponPool {
        kind,
        stored_ids: Vec::new(),
        draw_ids: Vec::new(),
        sources: Vec::new(),
        source_sha256: generated::RSDB_SHA256,
    }
}

fn append_empty_fallback(pool: &mut EffectiveWeaponPool, signed_capacity: i32, id: u16) {
    if signed_capacity > 0 && pool.draw_ids.is_empty() {
        pool.stored_ids.push(i32::from(id));
        pool.draw_ids.push(id);
        pool.sources.push(EffectivePoolSource::Fallback);
    }
}

/// Source candidates for the helper-normal pool at `0x37b1e50..0x37b21e4`.
///
/// `season_cap` is the signed runtime value at this builder call. Both
/// builders read the same field but may observe it at different times.
/// This filters static RSDB rows only.
pub fn normal_source_candidates(season_cap: i32) -> RsdbOrderedCandidates {
    filter_source_rows(
        SourcePoolKind::Normal,
        generated::NORMAL_SOURCE_ROWS,
        season_cap,
        20040,
    )
}

/// Source candidates for the rare third pool at `0x37b6d50..0x37b70f4`.
///
/// Its signed cap comes from the same runtime field as the normal builder,
/// reread at this call, or zero when the object is absent. This filters
/// static RSDB rows only.
pub fn third_source_candidates(season_cap: i32) -> RsdbOrderedCandidates {
    filter_source_rows(
        SourcePoolKind::ThirdRare,
        generated::THIRD_SOURCE_ROWS,
        season_cap,
        20900,
    )
}

fn filter_source_rows(
    kind: SourcePoolKind,
    rows: &'static [SourceWeaponCandidate],
    season_cap: i32,
    conditional_empty_fallback_id: u16,
) -> RsdbOrderedCandidates {
    RsdbOrderedCandidates {
        kind,
        entries: rows
            .iter()
            .copied()
            .filter(|row| row.season <= season_cap)
            .collect(),
        source_sha256: generated::RSDB_SHA256,
        conditional_empty_fallback_id,
    }
}
