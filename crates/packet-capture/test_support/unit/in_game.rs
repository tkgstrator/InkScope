use super::*;
use inkscope::decode::{Field, Row};
use inkscope::pcap::UdpPacket;

fn packet(record: u64) -> UdpPacket {
    UdpPacket {
        record,
        timestamp_ns: record * 1_000_000,
        source: "192.0.2.1".parse().unwrap(),
        destination: "192.0.2.2".parse().unwrap(),
        source_port: 49152,
        destination_port: 49152,
        source_mac: [1; 6],
        destination_mac: [2; 6],
        payload: vec![],
    }
}

fn field(name: &str, value: impl ToString) -> Field {
    Field {
        name: name.into(),
        value: value.to_string(),
        bits: None,
    }
}

fn row(packet: &UdpPacket, kind: &str, protocol: u8, fields: Vec<Field>) -> Row {
    Row {
        record: packet.record,
        timestamp_ns: packet.timestamp_ns,
        source: format!("{}:{}", packet.source, packet.source_port),
        destination: format!("{}:{}", packet.destination, packet.destination_port),
        source_mac: String::new(),
        destination_mac: String::new(),
        direction: None,
        kind: kind.into(),
        status: "authenticated".into(),
        layout: Some("confirmed_fields".into()),
        protocol: Some(protocol),
        port: Some(if protocol == 5 { 101 } else { 100 }),
        chunk: Some(0),
        fields,
        authenticated_payload_hex: None,
        wave_preview: None,
    }
}

fn typed_payload(length: usize, type_id: u32, identity: u8) -> String {
    let mut payload = vec![0u8; length];
    payload[0] = 7;
    payload[1..3].copy_from_slice(&((length - 8) as u16).to_be_bytes());
    payload[13..15].copy_from_slice(&((length - 17) as u16).to_be_bytes());
    payload[21..25].copy_from_slice(&type_id.to_le_bytes());
    payload[25] = identity;
    hex::encode(payload)
}

fn setting(packet: &UdpPacket, identity: u8) -> Row {
    let mut result = row(
        packet,
        "OnlineCoopSetting",
        8,
        vec![
            field("PayloadBytes", 211),
            field("TypeId", "0x9a1e036d"),
            field("StageResource", "Cop_Shakedent"),
            field("JobType", 1),
            field("Difficulty", 25),
            field("GameFrame", 0),
            field("LifeNumber", 0),
            field("GameSeed[0]", 1),
            field("GameSeed[1]", 2),
            field("GameSeed[2]", 3),
            field("GameSeed[3]", 4),
        ],
    );
    result.authenticated_payload_hex = Some(typed_payload(211, 0x9a1e036d, identity));
    result
}

fn state(packet: &UdpPacket, sequence: u8) -> Row {
    row(
        packet,
        "CoopSequenceNetState",
        5,
        vec![
            field("PayloadBytes", 118),
            field("FrameMode", 1),
            field("FrameLength", 63),
            field("GroupId", 1104),
            field("Present", 1),
            field("Sequence", sequence),
        ],
    )
}

fn finish(packet: &UdpPacket, identity: u8) -> Row {
    let mut result = row(
        packet,
        "CoopSeqRoundFinishEvent",
        8,
        vec![field("PayloadBytes", 55), field("TypeId", "0xc7cd3faf")],
    );
    result.layout = Some("type_only".into());
    result.authenticated_payload_hex = Some(typed_payload(55, 0xc7cd3faf, identity));
    result
}

fn arm_and_start(gate: &mut Gate, packet: &UdpPacket) {
    assert!(gate.observe(packet, &[setting(packet, 1)]).start_session);
    assert_eq!(
        gate.observe(packet, &[state(packet, 0)]).video,
        VideoAction::Start
    );
}

#[test]
fn authenticated_setting_arms_but_only_valid_state_starts_video() {
    let mut gate = Gate::default();
    let packet = packet(1);
    assert_eq!(gate.observe(&packet, &[]), Decision::default());
    let armed = gate.observe(&packet, &[setting(&packet, 1)]);
    assert!(armed.start_session && armed.save_packet);
    assert_eq!(armed.video, VideoAction::None);
    assert_eq!(armed.close_session, None);
    let started = gate.observe(&packet, &[state(&packet, 0)]);
    assert!(started.save_packet && !started.start_session);
    assert_eq!(started.video, VideoAction::Start);
}

#[test]
fn malformed_or_unauthenticated_triggers_do_not_arm() {
    let packet = packet(1);
    for variant in 0..7 {
        let mut gate = Gate::default();
        let mut candidate = setting(&packet, 1);
        match variant {
            0 => candidate.status = "authentication_failed".into(),
            1 => candidate.protocol = Some(11),
            2 => candidate.layout = Some("type_fields".into()),
            3 => candidate.fields.retain(|f| f.name != "GameSeed[3]"),
            4 => {
                candidate
                    .fields
                    .iter_mut()
                    .find(|f| f.name == "PayloadBytes")
                    .unwrap()
                    .value = "210".into()
            },
            5 => candidate.authenticated_payload_hex = None,
            6 => candidate.port = Some(101),
            _ => unreachable!(),
        }
        assert_eq!(
            gate.observe(&packet, &[candidate]),
            Decision::default(),
            "variant {variant}"
        );
    }
}

#[test]
fn finishing_pauses_until_a_new_sequence_and_retransmission_is_ignored() {
    let mut gate = Gate::default();
    let packet = packet(1);
    arm_and_start(&mut gate, &packet);
    let stopped = gate.observe(&packet, &[finish(&packet, 10)]);
    assert_eq!(stopped.video, VideoAction::Stop);
    assert!(stopped.save_packet);
    assert!(!gate.observe(&packet, &[state(&packet, 0)]).save_packet);
    assert_eq!(
        gate.observe(&packet, &[state(&packet, 0)]).video,
        VideoAction::None
    );
    assert_eq!(
        gate.observe(&packet, &[state(&packet, 1)]).video,
        VideoAction::Start
    );
    assert_eq!(
        gate.observe(&packet, &[finish(&packet, 10)]).video,
        VideoAction::None
    );
    assert!(gate.observe(&packet, &[]).save_packet);
    assert_eq!(
        gate.observe(&packet, &[finish(&packet, 11)]).video,
        VideoAction::Stop
    );
}

#[test]
fn same_setting_is_deduplicated_but_new_identity_rotates_even_with_same_seed() {
    let mut gate = Gate::default();
    let packet = packet(1);
    arm_and_start(&mut gate, &packet);
    assert!(!gate.observe(&packet, &[setting(&packet, 1)]).start_session);
    let rotated = gate.observe(&packet, &[setting(&packet, 2)]);
    assert!(rotated.start_session && rotated.save_packet);
    assert_eq!(rotated.video, VideoAction::Stop);
    assert_eq!(rotated.close_session, Some("new_setting"));
    assert!(!gate.observe(&packet, &[setting(&packet, 1)]).start_session);
}

#[test]
fn another_room_cannot_start_stop_or_leak_packets_into_the_active_session() {
    let mut gate = Gate::default();
    let packet = packet(1);
    arm_and_start(&mut gate, &packet);
    let mut other = packet.clone();
    other.source = "192.0.2.3".parse().unwrap();
    assert_eq!(
        gate.observe(&other, &[setting(&other, 2)]),
        Decision::default()
    );
    assert_eq!(
        gate.observe(&other, &[finish(&other, 2)]),
        Decision::default()
    );
    assert_eq!(gate.observe(&other, &[]), Decision::default());
    assert!(gate.observe(&packet, &[]).save_packet);
}

#[test]
fn session_ack_is_not_end_and_explicit_lobby_closes_the_matching_room() {
    let mut gate = Gate::default();
    let packet = packet(1);
    arm_and_start(&mut gate, &packet);
    let ack = row(&packet, "Session 6", 13, vec![]);
    assert_eq!(gate.observe(&packet, &[ack]).close_session, None);
    let lobby = row(
        &packet,
        "PrivateSetting",
        11,
        vec![field("PayloadBytes", 78), field("StageId", "")],
    );
    let closed = gate.observe(&packet, &[lobby]);
    assert_eq!(closed.video, VideoAction::Stop);
    assert_eq!(closed.close_session, Some("lobby_setting"));
    assert!(!closed.save_packet);
    assert_eq!(gate.observe(&packet, &[]), Decision::default());
}

#[test]
fn reset_and_early_finish_leave_no_video_or_session_active() {
    let mut gate = Gate::default();
    let packet = packet(1);
    arm_and_start(&mut gate, &packet);
    gate.observe(&packet, &[finish(&packet, 1)]);
    assert_eq!(
        gate.observe(&packet, &[state(&packet, 0)]).video,
        VideoAction::None
    );
    gate.reset();
    assert_eq!(
        gate.observe(&packet, &[state(&packet, 1)]),
        Decision::default()
    );
    assert!(gate.observe(&packet, &[setting(&packet, 1)]).start_session);
}

#[test]
fn invalid_state_and_finish_shapes_do_not_change_video() {
    let mut gate = Gate::default();
    let packet = packet(1);
    gate.observe(&packet, &[setting(&packet, 1)]);
    let mut bad_state = state(&packet, 0);
    bad_state
        .fields
        .iter_mut()
        .find(|f| f.name == "FrameLength")
        .unwrap()
        .value = "62".into();
    assert_eq!(gate.observe(&packet, &[bad_state]).video, VideoAction::None);
    gate.observe(&packet, &[state(&packet, 0)]);
    let mut bad_finish = finish(&packet, 3);
    bad_finish
        .fields
        .iter_mut()
        .find(|f| f.name == "PayloadBytes")
        .unwrap()
        .value = "54".into();
    assert_eq!(
        gate.observe(&packet, &[bad_finish]).video,
        VideoAction::None
    );
    assert!(gate.observe(&packet, &[]).save_packet);
}

#[test]
fn finish_before_first_sequence_does_not_reopen_on_residual_state() {
    let mut gate = Gate::default();
    let packet = packet(1);
    gate.observe(&packet, &[setting(&packet, 1)]);
    gate.observe(&packet, &[finish(&packet, 10)]);
    assert_eq!(
        gate.observe(&packet, &[state(&packet, 0)]),
        Decision::default()
    );
    assert_eq!(
        gate.observe(&packet, &[state(&packet, 1)]),
        Decision::default()
    );
    gate.observe(&packet, &[setting(&packet, 2)]);
    assert_eq!(
        gate.observe(&packet, &[state(&packet, 0)]).video,
        VideoAction::Start
    );
}
